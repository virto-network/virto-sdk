//! JSON-RPC protocol layer.
//!
//! Provides the [`Rpc`] trait for request/response, [`RpcSubscription`] for
//! subscription-capable transports, and concrete backend implementations.
//!
//! All trait methods take `&mut self` — single-threaded, no spawning.
//! Params and results are raw JSON strings — no `serde_json::Value`.

#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
use core::fmt::Write;
use serde::Deserialize;

use crate::prelude::*;

/// Hex-encode bytes with `0x` prefix into an existing String, avoiding a new allocation.
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
fn push_hex(buf: &mut String, bytes: &[u8]) {
    buf.push_str("0x");
    for &b in bytes {
        let _ = write!(buf, "{:02x}", b);
    }
}

/// Hex-encode bytes with `0x` prefix, returning a new String.
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    push_hex(&mut s, bytes);
    s
}

// --- JSON-RPC message types ---

/// Format a JSON-RPC request into `buf`. Returns the written length.
/// `params` is a pre-serialized JSON string (e.g. `"[]"` or `"[true]"`).
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
pub(crate) fn format_request(buf: &mut String, id: u32, method: &str, params: &str) {
    buf.clear();
    let _ = write!(
        buf,
        r#"{{"jsonrpc":"2.0","id":{},"method":"{}","params":{}}}"#,
        id, method, params
    );
}

/// A JSON-RPC notification (subscription event) — has `method` and `params` but no `id`.
#[derive(Debug)]
pub struct Notification {
    pub method: String,
    pub params: NotificationParams,
}

#[derive(Debug)]
pub struct NotificationParams {
    pub subscription: String,
    /// Raw JSON of the notification result (not parsed).
    pub result: String,
}

/// Parsed JSON-RPC response fields extracted from raw JSON.
#[derive(Debug)]
pub struct RpcResponse {
    pub id: u32,
    /// Raw JSON of the result field (e.g. `"\"0x1234\""` or `"{\"result\":\"started\",...}"`).
    pub result: Option<String>,
}

/// Represents either a response (has `id`) or a notification (has `method` + `params.subscription`).
#[derive(Debug)]
pub enum IncomingMessage {
    Response(RpcResponse),
    Error(JsonRpcError),
    Notification(Notification),
}

impl IncomingMessage {
    /// Parse a JSON string into either a Response, Error, or Notification.
    /// All parsing uses lightweight string scanning — no serde_json.
    pub fn parse(json: &str) -> Option<Self> {
        // Check for error first
        if json.contains("\"error\"") && json.contains("\"id\"") {
            if let (Some(code), Some(message)) = (
                extract_json_number(json, "\"code\":"),
                extract_json_string(json, "\"message\":\""),
            ) {
                return Some(IncomingMessage::Error(JsonRpcError { code, message }));
            }
        }

        // Response: has "id" and "result"
        if json.contains("\"id\"") && !json.contains("\"method\"") {
            let id = extract_json_number(json, "\"id\":")?;
            let result = extract_json_object(json, "\"result\":");
            return Some(IncomingMessage::Response(RpcResponse {
                id: id as u32,
                result,
            }));
        }

        // Notification: has "method" and "params.subscription"
        if json.contains("\"method\"") {
            let subscription = extract_json_string(json, "\"subscription\":\"")?;
            let result = extract_json_object(json, "\"result\":")?;
            return Some(IncomingMessage::Notification(Notification {
                method: extract_json_string(json, "\"method\":\"")?,
                params: NotificationParams {
                    subscription,
                    result,
                },
            }));
        }

        // Ambiguous — try as response
        if json.contains("\"id\"") {
            let id = extract_json_number(json, "\"id\":")?;
            let result = extract_json_object(json, "\"result\":");
            return Some(IncomingMessage::Response(RpcResponse {
                id: id as u32,
                result,
            }));
        }

        None
    }
}

/// Extract a JSON string value by scanning for a `"key":"value"` pattern.
///
/// Safe for values that don't contain escape sequences (hex hashes,
/// identifiers, operation IDs). The marker must end with `"` to anchor
/// at the start of the value string.
pub(crate) fn extract_json_str<'a>(json: &'a str, marker: &str) -> Option<&'a str> {
    let start = json.find(marker)? + marker.len();
    let end = json[start..].find('"')?;
    Some(&json[start..start + end])
}

/// Like [`extract_json_str`] but returns an owned String.
/// Use when the result must outlive the input (e.g. buffered notifications).
fn extract_json_string(json: &str, marker: &str) -> Option<String> {
    extract_json_str(json, marker).map(Into::into)
}

/// Extract a JSON number after a marker.
fn extract_json_number(json: &str, marker: &str) -> Option<i64> {
    let start = json.find(marker)? + marker.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Extract a raw JSON object/value after a key marker.
/// Handles nested braces to find the correct end.
pub(crate) fn extract_json_object(json: &str, marker: &str) -> Option<String> {
    let start = json.find(marker)? + marker.len();
    let rest = &json[start..];

    let first = rest.trim_start().chars().next()?;
    let rest = &json[start + (rest.len() - rest.trim_start().len())..];
    match first {
        '{' => {
            let mut depth = 0i32;
            let mut in_string = false;
            let mut escape = false;
            for (i, ch) in rest.char_indices() {
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' && in_string {
                    escape = true;
                    continue;
                }
                if ch == '"' {
                    in_string = !in_string;
                }
                if !in_string {
                    if ch == '{' {
                        depth += 1;
                    }
                    if ch == '}' {
                        depth -= 1;
                        if depth == 0 {
                            return Some(rest[..=i].into());
                        }
                    }
                }
            }
            None
        }
        '[' => {
            let mut depth = 0i32;
            let mut in_string = false;
            let mut escape = false;
            for (i, ch) in rest.char_indices() {
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' && in_string {
                    escape = true;
                    continue;
                }
                if ch == '"' {
                    in_string = !in_string;
                }
                if !in_string {
                    if ch == '[' {
                        depth += 1;
                    }
                    if ch == ']' {
                        depth -= 1;
                        if depth == 0 {
                            return Some(rest[..=i].into());
                        }
                    }
                }
            }
            None
        }
        '"' => {
            let end = rest[1..].find('"').map(|i| i + 2)?;
            Some(rest[..end].into())
        }
        _ => {
            let end = rest
                .find(|c: char| c == ',' || c == '}' || c == ']')
                .unwrap_or(rest.len());
            Some(rest[..end].trim().into())
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

/// Extract and hex-decode the `"result":"0x..."` value from raw JSON-RPC text.
#[cfg(feature = "ws")]
pub(crate) fn extract_hex_result(json: &str) -> Result<crate::prelude::Vec<u8>, JsonRpcError> {
    // Check for error first
    if json.contains("\"error\"") {
        if let (Some(code), Some(message)) = (
            extract_json_number(json, "\"code\":"),
            extract_json_string(json, "\"message\":\""),
        ) {
            return Err(JsonRpcError { code, message });
        }
    }

    let marker = "\"result\":\"0x";
    let start = json
        .find(marker)
        .ok_or_else(|| JsonRpcError::new(-32603, "no result in response"))?;
    let hex_start = start + marker.len();

    let hex_end = json[hex_start..]
        .find('"')
        .ok_or_else(|| JsonRpcError::new(-32603, "unterminated hex string"))?;

    let hex_str = &json[hex_start..hex_start + hex_end];
    hex::decode(hex_str).map_err(|_| JsonRpcError::new(-32603, "invalid hex"))
}

/// Helper: extract result as a JSON string value (strips quotes).
pub(crate) fn result_as_str(result: &str) -> Option<&str> {
    let trimmed = result.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        Some(&trimmed[1..trimmed.len() - 1])
    } else {
        None
    }
}

// --- Rpc trait ---

/// Async JSON-RPC request/response interface.
/// `params` is a pre-serialized JSON string (e.g. `"[]"` or `"[\"sub\",\"hash\"]"`).
/// Returns the raw JSON `result` field as a String.
#[allow(async_fn_in_trait)]
pub trait Rpc {
    async fn rpc(&mut self, method: &str, params: &str) -> RpcResult<String>;
}

/// Backends that support JSON-RPC subscriptions (WebSocket, smoldot).
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
#[allow(async_fn_in_trait)]
pub trait RpcSubscription: Rpc {
    /// Subscribe to a method. Returns the subscription ID.
    async fn subscribe(&mut self, method: &str, params: &str) -> RpcResult<String>;

    /// Read the next subscription event, blocking until one arrives.
    /// Returns `(subscription_id, raw_json_result)`.
    async fn next_event(&mut self) -> Option<(String, String)>;

    /// Non-blocking: return a buffered event if available.
    fn try_next_event(&mut self) -> Option<(String, String)>;

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

/// ChainHead v1 session manager.
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
pub mod chainhead;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
    #[test]
    fn to_hex_encodes_bytes() {
        assert_eq!(super::to_hex(&[0xde, 0xad]), "0xdead");
    }

    #[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
    #[test]
    fn to_hex_empty() {
        assert_eq!(super::to_hex(&[]), "0x");
    }

    #[test]
    fn parse_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":"0x1234"}"#;
        let msg = IncomingMessage::parse(json).unwrap();
        match msg {
            IncomingMessage::Response(resp) => {
                assert_eq!(resp.id, 1);
                assert_eq!(resp.result.unwrap(), "\"0x1234\"");
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
    fn parse_error() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"bad"}}"#;
        let msg = IncomingMessage::parse(json).unwrap();
        match msg {
            IncomingMessage::Error(e) => {
                assert_eq!(e.code, -32603);
                assert_eq!(e.message, "bad");
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(IncomingMessage::parse("not json at all").is_none());
    }

    #[test]
    fn result_as_str_strips_quotes() {
        assert_eq!(result_as_str("\"hello\""), Some("hello"));
        assert_eq!(result_as_str("123"), None);
        assert_eq!(result_as_str("null"), None);
    }
}
