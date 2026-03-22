//! JSON-RPC protocol layer.
//!
//! Provides the [`Rpc`] trait for request/response, [`RpcSubscription`] for
//! subscription-capable transports, and concrete backend implementations.
//!
//! All trait methods take `&mut self` — single-threaded, no spawning.

use core::fmt::Write;
use serde::{Deserialize, Serialize};

use crate::prelude::*;

/// Hex-encode bytes with `0x` prefix into an existing String, avoiding a new allocation.
fn push_hex(buf: &mut String, bytes: &[u8]) {
    buf.push_str("0x");
    for &b in bytes {
        let _ = write!(buf, "{:02x}", b);
    }
}

/// Hex-encode bytes with `0x` prefix, returning a new String.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    push_hex(&mut s, bytes);
    s
}

// --- Inline JSON-RPC protocol types ---

#[derive(Serialize)]
pub struct JsonRpcRequest<'a> {
    pub jsonrpc: &'a str,
    pub id: u32,
    pub method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct JsonRpcResponse {
    pub id: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn into_result(self) -> Result<serde_json::Value, JsonRpcError> {
        if let Some(err) = self.error {
            return Err(err);
        }
        self.result
            .ok_or_else(|| JsonRpcError::new(-1, "no result"))
    }
}

/// A JSON-RPC notification (subscription event) — has `method` and `params` but no `id`.
#[derive(Deserialize, Debug)]
pub struct Notification {
    pub method: String,
    pub params: NotificationParams,
}

#[derive(Deserialize, Debug)]
pub struct NotificationParams {
    pub subscription: String,
    pub result: serde_json::Value,
}

/// Represents either a response (has `id`) or a notification (has `method` + `params.subscription`).
#[derive(Debug)]
pub enum IncomingMessage {
    Response(JsonRpcResponse),
    Notification(Notification),
}

impl IncomingMessage {
    /// Parse a JSON string into either a Response or Notification.
    pub fn parse(json: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Raw {
            id: Option<serde_json::Value>,
            result: Option<serde_json::Value>,
            error: Option<JsonRpcError>,
            method: Option<String>,
            params: Option<serde_json::Value>,
        }

        let raw: Raw = serde_json::from_str(json).ok()?;

        if raw.id.as_ref().is_some_and(|v| !v.is_null()) || raw.method.is_none() {
            Some(IncomingMessage::Response(JsonRpcResponse {
                id: raw.id,
                result: raw.result,
                error: raw.error,
            }))
        } else {
            let params: NotificationParams = serde_json::from_value(raw.params?).ok()?;
            Some(IncomingMessage::Notification(Notification {
                method: raw.method?,
                params,
            }))
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcError {
    pub fn new(code: i64, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl core::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC error {}: {}", self.code, self.message)
    }
}

pub type RpcResult<T> = Result<T, JsonRpcError>;

// --- Rpc trait ---

/// Async JSON-RPC request/response interface.
#[allow(async_fn_in_trait)]
pub trait Rpc {
    async fn rpc(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> RpcResult<serde_json::Value>;
}

/// Backends that support JSON-RPC subscriptions (WebSocket, smoldot).
///
/// Events are buffered inside the transport. `rpc()` calls that encounter
/// subscription notifications while waiting for a response automatically
/// buffer them for later retrieval via `next_event`/`try_next_event`.
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
#[allow(async_fn_in_trait)]
pub trait RpcSubscription: Rpc {
    /// Subscribe to a method. Returns the subscription ID.
    async fn subscribe(&mut self, method: &str, params: serde_json::Value) -> RpcResult<String>;

    /// Read the next subscription event, blocking until one arrives.
    async fn next_event(&mut self) -> Option<serde_json::Value>;

    /// Non-blocking: return a buffered event if available.
    fn try_next_event(&mut self) -> Option<serde_json::Value>;

    /// Unsubscribe from a subscription.
    async fn unsubscribe(&mut self, method: &str, sub_id: &str) -> RpcResult<()>;
}

// --- Transport backends ---

#[cfg(feature = "ws-edge")]
pub mod edge;
#[cfg(feature = "smoldot")]
pub mod smoldot;
#[cfg(feature = "ws")]
pub mod ws;

/// ChainHead v1 session manager — public for embedded users who construct
/// `ChainHead<edge::Backend<T>>` directly.
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
pub mod chainhead;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_hex_encodes_bytes() {
        assert_eq!(to_hex(&[0xde, 0xad]), "0xdead");
    }

    #[test]
    fn to_hex_empty() {
        assert_eq!(to_hex(&[]), "0x");
    }

    #[test]
    fn parse_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":"0x1234"}"#;
        let msg = IncomingMessage::parse(json).unwrap();
        match msg {
            IncomingMessage::Response(resp) => {
                assert_eq!(resp.id, Some(serde_json::json!(1)));
                assert_eq!(resp.result, Some(serde_json::json!("0x1234")));
            }
            _ => panic!("expected Response"),
        }
    }

    #[test]
    fn parse_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"chainHead_v1_followEvent","params":{"subscription":"sub1","result":{"event":"initialized","finalizedBlockHashes":["0xabc"]}}}"#;
        let msg = IncomingMessage::parse(json).unwrap();
        match msg {
            IncomingMessage::Notification(notif) => {
                assert_eq!(notif.method, "chainHead_v1_followEvent");
                assert_eq!(notif.params.subscription, "sub1");
            }
            _ => panic!("expected Notification"),
        }
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(IncomingMessage::parse("not json at all").is_none());
    }

    #[test]
    fn response_into_result_ok() {
        let resp = JsonRpcResponse {
            id: Some(serde_json::json!(1)),
            result: Some(serde_json::json!("ok")),
            error: None,
        };
        let val = resp.into_result().unwrap();
        assert_eq!(val, serde_json::json!("ok"));
    }

    #[test]
    fn response_into_result_error() {
        let resp = JsonRpcResponse {
            id: Some(serde_json::json!(1)),
            result: None,
            error: Some(JsonRpcError::new(-1, "fail")),
        };
        let err = resp.into_result().unwrap_err();
        assert_eq!(err.code, -1);
        assert_eq!(err.message, "fail");
    }
}
