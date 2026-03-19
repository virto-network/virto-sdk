//! Smoldot light client backend — no_std compatible.
//!
//! Generic over [`PlatformRef`](smoldot_light::platform::PlatformRef) so it works
//! on std (via `DefaultPlatform`) or embedded (custom platform).
//!
//! ```rust,ignore
//! // std — convenience constructors
//! let chain = Sube::connect_light(include_str!("chain_spec.json")).await?;
//!
//! // no_std — bring your own platform + spawn
//! let backend = smoldot::Backend::new(platform, spec, None, my_spawn)?;
//! let chain = RpcClient(backend);
//! ```

use alloc::{collections::BTreeMap, format, sync::Arc, vec::Vec};
use core::future::Future;
use core::sync::atomic::{AtomicU32, Ordering};

use futures_channel::oneshot;
use no_std_async::Mutex;
use serde::Deserialize;
use smoldot_light::platform::PlatformRef;
use smoldot_light::{AddChainConfig, AddChainConfigJsonRpc, Client};

use crate::rpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, Rpc, RpcResult};
use crate::Error;

type Id = u32;

/// Light client backend powered by smoldot.
///
/// Generic over the smoldot platform — use `DefaultPlatform` on std
/// or provide a custom implementation for embedded targets.
pub struct Backend<P: PlatformRef> {
    client: Arc<Mutex<Client<P, ()>>>,
    chain_id: smoldot_light::ChainId,
    pending: Arc<Mutex<BTreeMap<Id, oneshot::Sender<JsonRpcResponse>>>>,
    next_id: AtomicU32,
}

impl<P: PlatformRef> Backend<P> {
    /// Create a light client backend with a caller-provided platform and spawn function.
    ///
    /// `spawn` must run the given future to completion in the background.
    /// On std this is typically `async_std::task::spawn` or `tokio::spawn`.
    /// On embedded it could be an executor-specific spawn.
    pub fn new(
        platform: P,
        chain_spec: &str,
        relay_spec: Option<&str>,
        spawn: impl FnOnce(core::pin::Pin<alloc::boxed::Box<dyn Future<Output = ()> + Send>>),
    ) -> Result<Self, Error> {
        let mut client = Client::new(platform);

        // If relay chain spec is provided, add it first
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
        let mut responses = result
            .json_rpc_responses
            .ok_or(Error::Node("JSON-RPC not enabled".into()))?;

        let pending: Arc<Mutex<BTreeMap<Id, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));

        // Spawn background task to read responses and dispatch to waiting callers
        let pending_clone = pending.clone();
        spawn(alloc::boxed::Box::pin(async move {
            while let Some(json) = responses.next().await {
                log::trace!("smoldot response: {}", &json);
                let res: JsonRpcResponse = match serde_json::from_str(&json) {
                    Ok(r) => r,
                    Err(e) => {
                        log::warn!("smoldot: failed to parse response: {e}");
                        continue;
                    }
                };
                if let Some(id) = res.id.as_u64() {
                    let id = id as Id;
                    let mut map = pending_clone.lock().await;
                    if let Some(sender) = map.remove(&id) {
                        if let Err(res) = sender.send(res) {
                            log::warn!("smoldot: dropped response for id {}: {:?}", id, res);
                        }
                    }
                }
            }
            log::info!("smoldot: response reader stopped");
        }));

        let client = Arc::new(Mutex::new(client));

        Ok(Backend {
            client,
            chain_id,
            pending,
            next_id: AtomicU32::new(1),
        })
    }
}

/// Convenience constructors when std is available.
#[cfg(feature = "std")]
impl Backend<Arc<smoldot_light::platform::DefaultPlatform>> {
    /// Start a light client using the default std platform.
    pub fn new_std(chain_spec: &str) -> Result<Self, Error> {
        Self::new_std_with_relay(chain_spec, None)
    }

    /// Start a parachain light client using the default std platform.
    pub fn new_std_with_relay(chain_spec: &str, relay_spec: Option<&str>) -> Result<Self, Error> {
        let platform = smoldot_light::platform::DefaultPlatform::new(
            env!("CARGO_PKG_NAME").into(),
            env!("CARGO_PKG_VERSION").into(),
        );
        Self::new(platform, chain_spec, relay_spec, |fut| {
            async_std::task::spawn(fut);
        })
    }
}

impl<P: PlatformRef> Rpc for Backend<P> {
    async fn rpc<T>(&self, method: &str, params: &[&str]) -> RpcResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        log::info!("smoldot RPC `{}` (ID={})", method, id);

        let (sender, receiver) = oneshot::channel::<JsonRpcResponse>();
        self.pending.lock().await.insert(id, sender);

        let request_json = serde_json::to_string(&JsonRpcRequest {
            id,
            jsonrpc: "2.0",
            method,
            params: Some(Self::build_params(params)),
        })
        .map_err(|e| JsonRpcError::new(-32700, &e.to_string()))?;

        log::debug!("smoldot request: {}", &request_json);

        self.client
            .lock()
            .await
            .json_rpc_request(request_json, self.chain_id)
            .map_err(|e| JsonRpcError::new(-32603, &format!("{e}")))?;

        let response = receiver
            .await
            .map_err(|_| JsonRpcError::new(-32603, "response channel closed"))?;

        response.into_result()
    }
}
