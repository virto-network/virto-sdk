//! Smoldot light client transport — no_std compatible, no spawning.
//!
//! # Browser / wasm32 status
//!
//! This module is std-only because `smoldot-light` 0.19's only shipped
//! `PlatformRef` implementation (`DefaultPlatform`) depends on
//! `std::thread`, `std::net`, `std::time::Instant`, and `UNIX_EPOCH`,
//! none of which compile on `wasm32-unknown-unknown`.
//!
//! Running a smoldot light client inside the browser from Rust/wasm
//! requires a custom `PlatformRef` that bridges to browser APIs
//! (`web-sys::WebSocket`, `gloo-timers`, `getrandom/js`, ...). That is
//! tracked as future work; browser applications should use the
//! `ws-web` backend against a public RPC endpoint today.

use alloc::collections::VecDeque;
use alloc::{format, string::String, vec::Vec};

use smoldot_light::platform::PlatformRef;
use smoldot_light::{AddChainConfig, AddChainConfigJsonRpc, Client};

use super::{IncomingMessage, JsonRpcError, RequestCleanup, Rpc, RpcResult};
use crate::Error;

#[derive(Clone, Debug)]
struct PendingRequest {
    id: u32,
    cleanup: RequestCleanup,
    is_cleanup_request: bool,
}

/// Light client backend powered by smoldot.
pub struct Backend<P: PlatformRef> {
    // Keep the client before the managed runtime guard: Rust drops struct
    // fields in declaration order, so every smoldot task is cancelled before
    // the guard stops and joins its executor threads.
    client: Client<P, ()>,
    chain_id: smoldot_light::ChainId,
    responses: smoldot_light::JsonRpcResponses<P>,
    event_buffer: VecDeque<(String, String)>,
    next_id: u32,
    /// A request remains installed across cancellation of the future that was
    /// waiting for its response. The next explicit cleanup or RPC call drains
    /// that exact response before the transport can be reused.
    pending_request: Option<PendingRequest>,
    /// Exact cleanup RPC retained until a success response is observed. This
    /// makes cleanup retryable even when its own future is cancelled or fails.
    pending_cleanup_retry: Option<(String, String)>,
    #[cfg(feature = "std")]
    managed_runtime: Option<super::managed_platform::RuntimeGuard>,
}

impl<P: PlatformRef> Backend<P> {
    /// Create a light client backend with a caller-provided platform.
    pub fn new(platform: P, chain_spec: &str, relay_spec: Option<&str>) -> Result<Self, Error> {
        let mut client = Client::new(platform);

        let relay_chain_ids: Vec<smoldot_light::ChainId> = if let Some(relay) = relay_spec {
            let relay_result = client
                .add_chain(AddChainConfig {
                    user_data: (),
                    specification: relay,
                    database_content: "",
                    potential_relay_chains: core::iter::empty(),
                    statement_protocol_config: None,
                    json_rpc: AddChainConfigJsonRpc::Disabled,
                })
                .map_err(|e| Error::Node(format!("relay chain: {e}")))?;
            alloc::vec![relay_result.chain_id]
        } else {
            alloc::vec![]
        };

        let result = client
            .add_chain(AddChainConfig {
                user_data: (),
                specification: chain_spec,
                database_content: "",
                potential_relay_chains: relay_chain_ids.into_iter(),
                statement_protocol_config: None,
                json_rpc: AddChainConfigJsonRpc::Enabled {
                    max_pending_requests: 128.try_into().expect("non-zero"),
                    max_subscriptions: 1024,
                },
            })
            .map_err(|e| Error::Node(format!("add chain: {e}")))?;

        let chain_id = result.chain_id;
        let responses = result
            .json_rpc_responses
            .ok_or(Error::Node("JSON-RPC not enabled".into()))?;

        Ok(Backend {
            client,
            chain_id,
            responses,
            event_buffer: VecDeque::new(),
            next_id: 1,
            pending_request: None,
            pending_cleanup_retry: None,
            #[cfg(feature = "std")]
            managed_runtime: None,
        })
    }
}

impl<P: PlatformRef> Backend<P> {
    fn start_request(
        &mut self,
        method: &str,
        params: &str,
        cleanup: RequestCleanup,
        is_cleanup_request: bool,
    ) -> RpcResult<u32> {
        if self.pending_request.is_some() {
            return Err(JsonRpcError::new(
                -32603,
                "cannot start RPC while a cancelled request is unresolved",
            ));
        }

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        log::info!("RPC `{}` (ID={})", method, id);

        let mut request = String::new();
        super::format_request(&mut request, id, method, params);
        log::debug!("RPC request: {}", &request);
        self.client
            .json_rpc_request(request, self.chain_id)
            .map_err(|error| {
                log::error!("smoldot send error: {error}");
                JsonRpcError::new(-32603, "send failed")
            })?;
        self.pending_request = Some(PendingRequest {
            id,
            cleanup,
            is_cleanup_request,
        });
        Ok(id)
    }

    async fn wait_for_response(&mut self, id: u32) -> RpcResult<String> {
        loop {
            let json = self
                .responses
                .next()
                .await
                .ok_or_else(|| JsonRpcError::new(-32603, "smoldot responses closed"))?;

            log::trace!("smoldot response: {}", &json);
            match IncomingMessage::parse(&json) {
                Some(IncomingMessage::Response(response)) if response.id == id => {
                    self.pending_request = None;
                    return response.result.ok_or_else(|| JsonRpcError {
                        id: Some(id),
                        code: -1,
                        message: "no result".into(),
                    });
                }
                Some(IncomingMessage::Error(error))
                    if error.id.is_none() || error.id == Some(id) =>
                {
                    self.pending_request = None;
                    return Err(error);
                }
                Some(IncomingMessage::Response(response)) => {
                    log::warn!("unexpected response id: {}", response.id);
                }
                Some(IncomingMessage::Error(error)) => {
                    log::warn!("unexpected error response id: {:?}", error.id);
                }
                Some(IncomingMessage::Notification(notification)) => {
                    self.event_buffer
                        .push_back((notification.params.subscription, notification.params.result));
                }
                None => log::warn!("failed to parse smoldot message: {}", &json),
            }
        }
    }

    async fn send_and_wait(
        &mut self,
        method: &str,
        params: &str,
        cleanup: RequestCleanup,
        is_cleanup_request: bool,
    ) -> RpcResult<String> {
        let id = self.start_request(method, params, cleanup, is_cleanup_request)?;
        self.wait_for_response(id).await
    }

    async fn apply_abandoned_cleanup(
        &mut self,
        cleanup: RequestCleanup,
        response: &str,
    ) -> RpcResult<()> {
        match cleanup {
            RequestCleanup::None => Ok(()),
            RequestCleanup::StopChainHeadOperation {
                follow_subscription,
            } => {
                let result = super::extract_json_str(response, "\"result\":\"");
                if result != Some("started") {
                    return Ok(());
                }
                let operation_id = super::extract_json_str(response, "\"operationId\":\"")
                    .ok_or_else(|| {
                        JsonRpcError::new(-32603, "started operation has no operation id")
                    })?;
                let params = format!(r#"["{}","{}"]"#, follow_subscription, operation_id);
                self.run_cleanup_request("chainHead_v1_stopOperation", &params)
                    .await
            }
            RequestCleanup::UnwatchTransaction => {
                let subscription_id = super::result_as_str(response).ok_or_else(|| {
                    JsonRpcError::new(-32603, "transaction watch has no subscription id")
                })?;
                let params = format!(r#"["{}"]"#, subscription_id);
                self.run_cleanup_request("transactionWatch_v1_unwatch", &params)
                    .await
            }
        }
    }

    async fn run_cleanup_request(&mut self, method: &str, params: &str) -> RpcResult<()> {
        self.pending_cleanup_retry = Some((method.into(), params.into()));
        let result = self
            .send_and_wait(method, params, RequestCleanup::None, true)
            .await
            .map(|_| ());
        if result.is_ok() {
            self.pending_cleanup_retry = None;
        }
        result
    }

    async fn reconcile_pending_request(&mut self) -> RpcResult<()> {
        let Some(pending) = self.pending_request.clone() else {
            return Ok(());
        };
        let response = match self.wait_for_response(pending.id).await {
            Ok(response) => response,
            Err(error) => {
                // A matching JSON-RPC error proves that the abandoned request
                // did not create server-side state. Transport failures leave
                // the pending request installed and must be surfaced.
                if self.pending_request.is_none() && !pending.is_cleanup_request {
                    return Ok(());
                }
                return Err(error);
            }
        };
        if pending.is_cleanup_request {
            self.pending_cleanup_retry = None;
            return Ok(());
        }
        self.apply_abandoned_cleanup(pending.cleanup, &response)
            .await
    }

    async fn reconcile_cleanup_retry(&mut self) -> RpcResult<()> {
        self.reconcile_pending_request().await?;
        let Some((method, params)) = self.pending_cleanup_retry.clone() else {
            return Ok(());
        };
        self.run_cleanup_request(&method, &params).await
    }
}

impl<P: PlatformRef> super::Rpc for Backend<P> {
    async fn rpc(&mut self, method: &str, params: &str) -> RpcResult<String> {
        self.reconcile_cleanup_retry().await?;
        self.send_and_wait(method, params, RequestCleanup::None, false)
            .await
    }

    async fn rpc_with_cleanup(
        &mut self,
        method: &str,
        params: &str,
        cleanup: RequestCleanup,
    ) -> RpcResult<String> {
        self.reconcile_cleanup_retry().await?;
        self.send_and_wait(method, params, cleanup, false).await
    }

    async fn cancel_pending_request(&mut self) -> RpcResult<()> {
        self.reconcile_cleanup_retry().await
    }
}

impl<P: PlatformRef> super::RpcSubscription for Backend<P> {
    async fn subscribe(&mut self, method: &str, params: &str) -> RpcResult<String> {
        let result = self.rpc(method, params).await?;
        super::result_as_str(&result)
            .map(|s| s.into())
            .ok_or_else(|| JsonRpcError::new(-32603, "expected string subscription id"))
    }

    async fn subscribe_with_cleanup(
        &mut self,
        method: &str,
        params: &str,
        cleanup: RequestCleanup,
    ) -> RpcResult<String> {
        let result = self.rpc_with_cleanup(method, params, cleanup).await?;
        super::result_as_str(&result)
            .map(Into::into)
            .ok_or_else(|| JsonRpcError::new(-32603, "expected string subscription id"))
    }

    async fn next_event(&mut self) -> Option<(String, String)> {
        if self.reconcile_cleanup_retry().await.is_err() {
            return None;
        }
        if let Some(event) = self.event_buffer.pop_front() {
            return Some(event);
        }
        loop {
            let json = self.responses.next().await?;
            log::trace!("smoldot response: {}", &json);
            match IncomingMessage::parse(&json) {
                Some(IncomingMessage::Notification(n)) => {
                    return Some((n.params.subscription, n.params.result));
                }
                Some(IncomingMessage::Response(_)) | Some(IncomingMessage::Error(_)) => {}
                None => {
                    log::warn!("failed to parse smoldot message: {}", &json);
                }
            }
        }
    }

    fn try_next_event(&mut self) -> Option<(String, String)> {
        self.event_buffer.pop_front()
    }

    async fn unsubscribe(&mut self, method: &str, sub_id: &str) -> RpcResult<()> {
        let params = format!(r#"["{}"]"#, sub_id);
        let _ = self.rpc(method, &params).await?;
        Ok(())
    }
}

/// Convenience constructors when std is available.
#[cfg(feature = "std")]
impl Backend<super::managed_platform::ManagedPlatform> {
    pub fn new_std(chain_spec: &str) -> Result<Self, Error> {
        Self::new_std_with_relay(chain_spec, None)
    }

    pub fn new_std_with_relay(chain_spec: &str, relay_spec: Option<&str>) -> Result<Self, Error> {
        let (platform, runtime) = super::managed_platform::RuntimeGuard::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            2,
        )
        .map_err(|error| Error::Node(format!("managed smoldot runtime: {error}")))?;
        let mut backend = Self::new(platform, chain_spec, relay_spec)?;
        backend.managed_runtime = Some(runtime);
        Ok(backend)
    }
}
