use crate::prelude::*;
use crate::rpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, Rpc, RpcResult};
use reqwest::Client;

/// HTTP-based JSON-RPC transport backed by `reqwest`.
#[derive(Debug)]
pub struct Backend(String);

impl Backend {
    pub fn new(url: String) -> Self {
        Backend(url)
    }
}

impl Rpc for Backend {
    async fn rpc(&self, method: &str, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        log::info!("RPC `{}` to {}", method, &self.0);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params: Some(params),
        };

        let res = Client::new()
            .post(&self.0)
            .json(&request)
            .send()
            .await
            .map_err(|err| JsonRpcError::new(-32000, &format!("transport: {err}")))?;

        let status = res.status();
        if status.is_success() {
            let response: JsonRpcResponse = res
                .json()
                .await
                .map_err(|err| JsonRpcError::new(-32700, &format!("parse: {err}")))?;
            response.into_result()
        } else {
            let err = res
                .text()
                .await
                .unwrap_or_else(|_| status.canonical_reason().unwrap_or("unknown").into());
            Err(if status.is_client_error() {
                JsonRpcError::new(-32600, &err)
            } else {
                JsonRpcError::new(-32603, &err)
            })
        }
    }
}
