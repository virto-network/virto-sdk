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
use alloc::{format, string::String, sync::Arc, vec::Vec};

use smoldot_light::platform::PlatformRef;
use smoldot_light::{AddChainConfig, AddChainConfigJsonRpc, Client};

use super::{IncomingMessage, JsonRpcError, Rpc, RpcResult};
use crate::Error;

/// Light client backend powered by smoldot.
pub struct Backend<P: PlatformRef> {
    client: Client<P, ()>,
    chain_id: smoldot_light::ChainId,
    responses: smoldot_light::JsonRpcResponses<P>,
    event_buffer: VecDeque<(String, String)>,
    next_id: u32,
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
        })
    }
}

impl<P: PlatformRef> super::Rpc for Backend<P> {
    async fn rpc(&mut self, method: &str, params: &str) -> RpcResult<String> {
        let id = self.next_id;
        self.next_id += 1;
        log::info!("RPC `{}` (ID={})", method, id);

        let mut req = String::new();
        super::format_request(&mut req, id, method, params);
        log::debug!("RPC request: {}", &req);

        self.client
            .json_rpc_request(req, self.chain_id)
            .map_err(|e| {
                log::error!("smoldot send error: {e}");
                JsonRpcError::new(-32603, "send failed")
            })?;

        loop {
            let json = self
                .responses
                .next()
                .await
                .ok_or_else(|| JsonRpcError::new(-32603, "smoldot responses closed"))?;

            log::trace!("smoldot response: {}", &json);

            match IncomingMessage::parse(&json) {
                Some(IncomingMessage::Response(r)) if r.id == id => {
                    return r.result.ok_or_else(|| JsonRpcError::new(-1, "no result"));
                }
                Some(IncomingMessage::Error(e)) => return Err(e),
                Some(IncomingMessage::Response(r)) => {
                    log::warn!("unexpected response id: {}", r.id);
                }
                Some(IncomingMessage::Notification(n)) => {
                    self.event_buffer
                        .push_back((n.params.subscription, n.params.result));
                }
                None => {
                    log::warn!("failed to parse smoldot message: {}", &json);
                }
            }
        }
    }
}

impl<P: PlatformRef> super::RpcSubscription for Backend<P> {
    async fn subscribe(&mut self, method: &str, params: &str) -> RpcResult<String> {
        let result = self.rpc(method, params).await?;
        super::result_as_str(&result)
            .map(|s| s.into())
            .ok_or_else(|| JsonRpcError::new(-32603, "expected string subscription id"))
    }

    async fn next_event(&mut self) -> Option<(String, String)> {
        if let Some(event) = self.event_buffer.pop_front() {
            return Some(event);
        }
        loop {
            let json = self.responses.next().await?;
            log::trace!("smoldot response: {}", &json);
            match IncomingMessage::parse(&json) {
                Some(IncomingMessage::Notification(n)) => {
                    return Some((n.params.subscription, n.params.result))
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
impl Backend<Arc<smoldot_light::platform::DefaultPlatform>> {
    pub fn new_std(chain_spec: &str) -> Result<Self, Error> {
        Self::new_std_with_relay(chain_spec, None)
    }

    pub fn new_std_with_relay(chain_spec: &str, relay_spec: Option<&str>) -> Result<Self, Error> {
        let platform = smoldot_light::platform::DefaultPlatform::new(
            env!("CARGO_PKG_NAME").into(),
            env!("CARGO_PKG_VERSION").into(),
        );
        Self::new(platform, chain_spec, relay_spec)
    }
}
