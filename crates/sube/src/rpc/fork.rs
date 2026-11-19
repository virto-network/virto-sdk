//! In-process fork transport: serves sube's JSON-RPC traits from a
//! [`sube_fork::ForkRpc`] session, so `ChainHead` runs against a local fork
//! exactly as against a node. No sockets, no spawning.

use alloc::string::String;

use sube_fork::{ForkRpc, RpcError};

use super::{JsonRpcError, Rpc, RpcResult, RpcSubscription};

fn rpc_error(e: RpcError) -> JsonRpcError {
    JsonRpcError::new(e.code, &e.message)
}

// `ForkRpc` has inherent methods of the same names; the calls below are to
// those, not recursive.

impl Rpc for ForkRpc {
    async fn rpc(&mut self, method: &str, params: &str) -> RpcResult<String> {
        self.request(method, params).map_err(rpc_error)
    }
}

impl RpcSubscription for ForkRpc {
    async fn subscribe(&mut self, method: &str, params: &str) -> RpcResult<String> {
        ForkRpc::subscribe(self, method, params).map_err(rpc_error)
    }

    async fn next_event(&mut self) -> Option<(String, String)> {
        Some(ForkRpc::next_event(self).await)
    }

    fn try_next_event(&mut self) -> Option<(String, String)> {
        ForkRpc::try_next_event(self)
    }

    async fn unsubscribe(&mut self, _method: &str, sub_id: &str) -> RpcResult<()> {
        ForkRpc::unsubscribe(self, sub_id);
        Ok(())
    }
}

/// Snapshots are downloaded from a full node over WebSocket.
#[cfg(feature = "ws")]
impl sube_fork::Transport for super::ws::Backend {
    async fn request(&mut self, method: &str, params: &str) -> Result<String, String> {
        self.rpc(method, params).await.map_err(|e| e.to_string())
    }
}
