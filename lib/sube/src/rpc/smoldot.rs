//! Smoldot light client transport — no_std compatible, no spawning.

use alloc::collections::VecDeque;
use alloc::{format, sync::Arc, vec::Vec};

use smoldot_light::platform::PlatformRef;
use smoldot_light::{AddChainConfig, AddChainConfigJsonRpc, Client};

use super::{IncomingMessage, JsonRpcError, JsonRpcRequest, Rpc, RpcResult};
use crate::Error;

/// Light client backend powered by smoldot.
pub struct Backend<P: PlatformRef> {
    client: Client<P, ()>,
    chain_id: smoldot_light::ChainId,
    responses: smoldot_light::JsonRpcResponses<P>,
    event_buffer: VecDeque<serde_json::Value>,
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
    async fn rpc(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> RpcResult<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        log::info!("RPC `{}` (ID={})", method, id);

        let msg = serde_json::to_string(&JsonRpcRequest {
            id,
            jsonrpc: "2.0",
            method,
            params: Some(params),
        })
        .expect("request is serializable");

        log::debug!("RPC request: {}", &msg);

        self.client
            .json_rpc_request(msg, self.chain_id)
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
                Some(IncomingMessage::Response(r))
                    if r.id.as_ref().and_then(|v| v.as_u64()) == Some(id as u64) =>
                {
                    return r.into_result();
                }
                Some(IncomingMessage::Response(r)) => {
                    log::warn!("unexpected response id: {:?}", r.id);
                }
                Some(IncomingMessage::Notification(n)) => {
                    self.event_buffer.push_back(n.params.result);
                }
                None => {
                    log::warn!("failed to parse smoldot message: {}", &json);
                }
            }
        }
    }
}

impl<P: PlatformRef> super::RpcSubscription for Backend<P> {
    async fn subscribe(&mut self, method: &str, params: serde_json::Value) -> RpcResult<String> {
        let sub_id: String = serde_json::from_value(self.rpc(method, params).await?)
            .map_err(|e| JsonRpcError::new(-32603, &format!("bad sub id: {e}")))?;
        Ok(sub_id)
    }

    async fn next_event(&mut self) -> Option<serde_json::Value> {
        if let Some(event) = self.event_buffer.pop_front() {
            return Some(event);
        }
        loop {
            let json = self.responses.next().await?;
            log::trace!("smoldot response: {}", &json);
            match IncomingMessage::parse(&json) {
                Some(IncomingMessage::Notification(n)) => return Some(n.params.result),
                Some(IncomingMessage::Response(r)) => {
                    log::warn!("unexpected response while waiting for event: {:?}", r.id);
                }
                None => {
                    log::warn!("failed to parse smoldot message: {}", &json);
                }
            }
        }
    }

    fn try_next_event(&mut self) -> Option<serde_json::Value> {
        self.event_buffer.pop_front()
    }

    async fn unsubscribe(&mut self, method: &str, sub_id: &str) -> RpcResult<()> {
        let _ = self.rpc(method, serde_json::json!([sub_id])).await?;
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
