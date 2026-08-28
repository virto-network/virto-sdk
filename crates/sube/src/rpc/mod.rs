//! JSON-RPC protocol layer.
//!
//! Provides the [`Rpc`] trait for request/response, [`RpcSubscription`] for
//! subscription-capable transports, and concrete backend implementations.
//!
//! All trait methods take `&mut self` — single-threaded, no spawning.
//! Params and results are raw JSON strings — no `serde_json::Value`.

#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
use core::fmt::Write;
use serde::Deserialize;

use crate::prelude::*;

/// Hex-encode bytes with `0x` prefix into an existing String, avoiding a new allocation.
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
fn push_hex(buf: &mut String, bytes: &[u8]) {
    buf.push_str("0x");
    for &b in bytes {
        let _ = write!(buf, "{:02x}", b);
    }
}

/// Hex-encode bytes with `0x` prefix, returning a new String.
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    push_hex(&mut s, bytes);
    s
}

// --- JSON-RPC message types ---

/// Format a JSON-RPC request into `buf`. Returns the written length.
/// `params` is a pre-serialized JSON string (e.g. `"[]"` or `"[true]"`).
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
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
    /// Parsing is structural and allocation-light, while result payloads remain raw JSON.
    pub fn parse(json: &str) -> Option<Self> {
        valid_json_value(json).then_some(())?;
        let id = object_field(json, "id").and_then(parse_json_i64);
        let method = object_field(json, "method");

        if let (Some(id), Some(error)) = (id, object_field(json, "error"))
            && let (Some(code), Some(message)) = (
                object_field(error, "code").and_then(parse_json_i64),
                object_field(error, "message").and_then(parse_json_string),
            )
        {
            let id = u32::try_from(id).ok()?;
            return Some(IncomingMessage::Error(JsonRpcError {
                id: Some(id),
                code,
                message,
            }));
        }

        if let (Some(id), None) = (id, method) {
            let id = u32::try_from(id).ok()?;
            let result = object_field(json, "result").map(Into::into);
            return Some(IncomingMessage::Response(RpcResponse { id, result }));
        }

        if let Some(method) = method {
            let params = object_field(json, "params")?;
            let subscription = object_field(params, "subscription").and_then(parse_json_string)?;
            let result = object_field(params, "result")?.into();
            return Some(IncomingMessage::Notification(Notification {
                method: parse_json_string(method)?,
                params: NotificationParams {
                    subscription,
                    result,
                },
            }));
        }

        None
    }
}

#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
fn marker_key(marker: &str) -> Option<&str> {
    marker
        .strip_prefix('"')?
        .split_once('"')
        .map(|(key, _)| key)
}

/// Extract an unescaped JSON string field without allocating.
///
/// This is used for identifiers and hashes. Escaped values return `None`;
/// callers that accept escaped text use [`extract_json_string`].
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
pub(crate) fn extract_json_str<'a>(json: &'a str, marker: &str) -> Option<&'a str> {
    let raw = object_field(json, marker_key(marker)?)?;
    let value = raw.strip_prefix('"')?.strip_suffix('"')?;
    (!value.as_bytes().contains(&b'\\')).then_some(value)
}

/// Like [`extract_json_str`] but returns an owned String.
/// Use when the result must outlive the input (e.g. buffered notifications).
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
fn extract_json_string(json: &str, marker: &str) -> Option<String> {
    object_field(json, marker_key(marker)?).and_then(parse_json_string)
}

/// Extract a raw JSON value for a top-level object field.
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
pub(crate) fn extract_json_object(json: &str, marker: &str) -> Option<String> {
    object_field(json, marker_key(marker)?).map(Into::into)
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        index += 1;
    }
    index
}

fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    (bytes.get(start) == Some(&b'"')).then_some(())?;
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index) {
        match byte {
            b'"' => return Some(index + 1),
            b'\\' => {
                index += 1;
                match bytes.get(index)? {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        for _ in 0..4 {
                            index += 1;
                            bytes.get(index)?.is_ascii_hexdigit().then_some(())?;
                        }
                    }
                    _ => return None,
                }
            }
            0x00..=0x1f => return None,
            _ => {}
        }
        index += 1;
    }
    None
}

fn value_end(bytes: &[u8], start: usize) -> Option<usize> {
    match *bytes.get(start)? {
        b'"' => string_end(bytes, start),
        b'{' | b'[' => {
            let mut stack = Vec::with_capacity(4);
            stack.push(bytes[start]);
            let mut index = start + 1;
            while let Some(byte) = bytes.get(index) {
                match byte {
                    b'"' => index = string_end(bytes, index)?,
                    b'{' | b'[' => {
                        stack.push(*byte);
                        index += 1;
                    }
                    b'}' if stack.last() == Some(&b'{') => {
                        stack.pop();
                        index += 1;
                        if stack.is_empty() {
                            return Some(index);
                        }
                    }
                    b']' if stack.last() == Some(&b'[') => {
                        stack.pop();
                        index += 1;
                        if stack.is_empty() {
                            return Some(index);
                        }
                    }
                    b'}' | b']' => return None,
                    _ => index += 1,
                }
            }
            None
        }
        _ => {
            let mut index = start;
            while bytes.get(index).is_some_and(|byte| {
                !matches!(byte, b',' | b'}' | b']') && !byte.is_ascii_whitespace()
            }) {
                index += 1;
            }
            (index > start).then_some(index)
        }
    }
}

fn object_field<'a>(json: &'a str, wanted: &str) -> Option<&'a str> {
    let bytes = json.as_bytes();
    let mut index = skip_whitespace(bytes, 0);
    (bytes.get(index) == Some(&b'{')).then_some(())?;
    index += 1;

    loop {
        index = skip_whitespace(bytes, index);
        if bytes.get(index) == Some(&b'}') {
            return None;
        }

        let key_end = string_end(bytes, index)?;
        let key = &json[index + 1..key_end - 1];
        index = skip_whitespace(bytes, key_end);
        (bytes.get(index) == Some(&b':')).then_some(())?;
        index = skip_whitespace(bytes, index + 1);

        let end = value_end(bytes, index)?;
        if key == wanted {
            return Some(&json[index..end]);
        }

        index = skip_whitespace(bytes, end);
        match bytes.get(index) {
            Some(b',') => index += 1,
            Some(b'}') => return None,
            _ => return None,
        }
    }
}

fn valid_json_value(json: &str) -> bool {
    let bytes = json.as_bytes();
    let start = skip_whitespace(bytes, 0);
    value_end(bytes, start)
        .map(|end| skip_whitespace(bytes, end) == bytes.len())
        .unwrap_or(false)
}

fn parse_json_i64(raw: &str) -> Option<i64> {
    raw.parse().ok()
}

fn parse_hex_quad(bytes: &[u8]) -> Option<u16> {
    if bytes.len() != 4 {
        return None;
    }
    bytes.iter().try_fold(0u16, |value, byte| {
        let digit = match byte {
            b'0'..=b'9' => u16::from(byte - b'0'),
            b'a'..=b'f' => u16::from(byte - b'a' + 10),
            b'A'..=b'F' => u16::from(byte - b'A' + 10),
            _ => return None,
        };
        value.checked_mul(16)?.checked_add(digit)
    })
}

fn parse_json_string(raw: &str) -> Option<String> {
    let body = raw.strip_prefix('"')?.strip_suffix('"')?;
    if !body.as_bytes().contains(&b'\\') {
        return Some(body.into());
    }

    let bytes = body.as_bytes();
    let mut output = String::with_capacity(body.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            let start = index;
            while index < bytes.len() && bytes[index] != b'\\' {
                index += 1;
            }
            output.push_str(core::str::from_utf8(&bytes[start..index]).ok()?);
            continue;
        }

        index += 1;
        match *bytes.get(index)? {
            b'"' => output.push('"'),
            b'\\' => output.push('\\'),
            b'/' => output.push('/'),
            b'b' => output.push('\u{0008}'),
            b'f' => output.push('\u{000c}'),
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b't' => output.push('\t'),
            b'u' => {
                let first = parse_hex_quad(bytes.get(index + 1..index + 5)?)?;
                index += 4;
                let codepoint = if (0xd800..=0xdbff).contains(&first) {
                    (bytes.get(index + 1..index + 3) == Some(&b"\\u"[..])).then_some(())?;
                    let second = parse_hex_quad(bytes.get(index + 3..index + 7)?)?;
                    (0xdc00..=0xdfff).contains(&second).then_some(())?;
                    index += 6;
                    0x10000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00)
                } else {
                    u32::from(first)
                };
                output.push(char::from_u32(codepoint)?);
            }
            _ => return None,
        }
        index += 1;
    }
    Some(output)
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonRpcError {
    /// JSON-RPC response identifier. Locally generated transport errors have
    /// no identifier and apply to the request currently being processed.
    #[serde(default)]
    pub id: Option<u32>,
    pub code: i64,
    pub message: String,
}

impl JsonRpcError {
    pub fn new(code: i64, message: &str) -> Self {
        Self {
            id: None,
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

/// Cleanup to perform if the future waiting for a request response is
/// cancelled after the request was accepted by the JSON-RPC service.
///
/// This is public only because it appears in the transport trait. Applications
/// should normally use the higher-level chainHead APIs instead.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub enum RequestCleanup {
    None,
    StopChainHeadOperation { follow_subscription: String },
    UnfollowChainHead,
    StopArchiveStorage,
    UnwatchTransaction,
}

/// Cancellation state shared by streaming transports whose send/read futures
/// can be dropped after bytes have reached the remote JSON-RPC service.
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    all(feature = "ws-web", target_arch = "wasm32")
))]
#[derive(Clone, Debug)]
pub(crate) struct PendingRequest {
    id: u32,
    cleanup: RequestCleanup,
    is_cleanup_request: bool,
}

#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    all(feature = "ws-web", target_arch = "wasm32")
))]
#[derive(Default)]
pub(crate) struct RequestTracker {
    pending_request: Option<PendingRequest>,
    pending_cleanup_retry: Option<(String, String)>,
}

/// Reusable cancellation-aware request machinery for WebSocket transports.
/// A request is installed before its send future is polled, so cancelling at
/// any await point leaves enough state to drain its response and tear down any
/// operation or subscription it created.
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    all(feature = "ws-web", target_arch = "wasm32")
))]
#[allow(async_fn_in_trait)]
pub(crate) trait TrackedTransport {
    fn request_tracker(&mut self) -> &mut RequestTracker;
    fn next_request_id(&mut self) -> &mut u32;
    fn event_buffer(&mut self) -> &mut alloc::collections::VecDeque<(String, String)>;

    async fn send_request_text(&mut self, request: &str) -> RpcResult<()>;
    async fn read_tracked_message(&mut self) -> RpcResult<IncomingMessage>;

    async fn start_request(
        &mut self,
        method: &str,
        params: &str,
        cleanup: RequestCleanup,
        is_cleanup_request: bool,
    ) -> RpcResult<u32> {
        if self.request_tracker().pending_request.is_some() {
            return Err(JsonRpcError::new(
                -32603,
                "cannot start RPC while a cancelled request is unresolved",
            ));
        }

        let id = *self.next_request_id();
        *self.next_request_id() = id.wrapping_add(1).max(1);
        log::info!("RPC `{}` (ID={})", method, id);

        let mut request = String::new();
        format_request(&mut request, id, method, params);
        log::debug!("RPC request: {}", &request);
        self.request_tracker().pending_request = Some(PendingRequest {
            id,
            cleanup,
            is_cleanup_request,
        });
        // Keep the request installed even when the transport reports a send
        // error. A streaming transport can fail after writing some or all of
        // the frame, so treating that error as proof the peer never observed
        // the request would make an operation leak possible. The next tracked
        // call must reconcile (or discard the connection) before proceeding.
        self.send_request_text(&request).await?;
        Ok(id)
    }

    async fn wait_for_response(&mut self, id: u32) -> RpcResult<String> {
        loop {
            match self.read_tracked_message().await? {
                IncomingMessage::Response(response) if response.id == id => {
                    self.request_tracker().pending_request = None;
                    return response.result.ok_or_else(|| JsonRpcError {
                        id: Some(id),
                        code: -1,
                        message: "no result".into(),
                    });
                }
                IncomingMessage::Error(error) if error.id.is_none() || error.id == Some(id) => {
                    self.request_tracker().pending_request = None;
                    return Err(error);
                }
                IncomingMessage::Response(response) => {
                    log::warn!("unexpected response id: {}", response.id);
                }
                IncomingMessage::Error(error) => {
                    log::warn!("unexpected error response id: {:?}", error.id);
                }
                IncomingMessage::Notification(notification) => {
                    self.event_buffer()
                        .push_back((notification.params.subscription, notification.params.result));
                }
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
        let id = self
            .start_request(method, params, cleanup, is_cleanup_request)
            .await?;
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
                let result = extract_json_str(response, "\"result\":\"");
                if result != Some("started") {
                    return Ok(());
                }
                let operation_id =
                    extract_json_str(response, "\"operationId\":\"").ok_or_else(|| {
                        JsonRpcError::new(-32603, "started operation has no operation id")
                    })?;
                let params = alloc::format!(r#"["{}","{}"]"#, follow_subscription, operation_id);
                self.run_cleanup_request("chainHead_v1_stopOperation", &params)
                    .await
            }
            RequestCleanup::UnfollowChainHead => {
                let subscription_id = result_as_str(response)
                    .ok_or_else(|| JsonRpcError::new(-32603, "follow has no subscription id"))?;
                let params = alloc::format!(r#"["{}"]"#, subscription_id);
                self.run_cleanup_request("chainHead_v1_unfollow", &params)
                    .await
            }
            RequestCleanup::StopArchiveStorage => {
                let subscription_id = result_as_str(response).ok_or_else(|| {
                    JsonRpcError::new(-32603, "archive storage has no subscription id")
                })?;
                let params = alloc::format!(r#"["{}"]"#, subscription_id);
                self.run_cleanup_request("archive_v1_stopStorage", &params)
                    .await
            }
            RequestCleanup::UnwatchTransaction => {
                let subscription_id = result_as_str(response).ok_or_else(|| {
                    JsonRpcError::new(-32603, "transaction watch has no subscription id")
                })?;
                let params = alloc::format!(r#"["{}"]"#, subscription_id);
                self.run_cleanup_request("transactionWatch_v1_unwatch", &params)
                    .await
            }
        }
    }

    async fn run_cleanup_request(&mut self, method: &str, params: &str) -> RpcResult<()> {
        self.request_tracker().pending_cleanup_retry = Some((method.into(), params.into()));
        let result = self
            .send_and_wait(method, params, RequestCleanup::None, true)
            .await
            .map(|_| ());
        if result.is_ok() {
            self.request_tracker().pending_cleanup_retry = None;
        }
        result
    }

    async fn reconcile_pending_request(&mut self) -> RpcResult<()> {
        let Some(pending) = self.request_tracker().pending_request.clone() else {
            return Ok(());
        };
        let response = match self.wait_for_response(pending.id).await {
            Ok(response) => response,
            Err(error) => {
                // A matching JSON-RPC error proves that the abandoned request
                // did not create server-side state. Transport failures retain
                // the request so the caller can discard the connection.
                if self.request_tracker().pending_request.is_none() && !pending.is_cleanup_request {
                    return Ok(());
                }
                return Err(error);
            }
        };
        if pending.is_cleanup_request {
            self.request_tracker().pending_cleanup_retry = None;
            return Ok(());
        }
        self.apply_abandoned_cleanup(pending.cleanup, &response)
            .await
    }

    async fn reconcile_cleanup_retry(&mut self) -> RpcResult<()> {
        self.reconcile_pending_request().await?;
        let Some((method, params)) = self.request_tracker().pending_cleanup_retry.clone() else {
            return Ok(());
        };
        self.run_cleanup_request(&method, &params).await
    }

    async fn tracked_rpc(&mut self, method: &str, params: &str) -> RpcResult<String> {
        self.reconcile_cleanup_retry().await?;
        self.send_and_wait(method, params, RequestCleanup::None, false)
            .await
    }

    async fn tracked_rpc_with_cleanup(
        &mut self,
        method: &str,
        params: &str,
        cleanup: RequestCleanup,
    ) -> RpcResult<String> {
        self.reconcile_cleanup_retry().await?;
        self.send_and_wait(method, params, cleanup, false).await
    }
}

/// Helper: extract result as a JSON string value (strips quotes).
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
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

    /// Submit a request whose successful response may create server-side
    /// state. Cancellation-aware transports retain enough information to tear
    /// that state down when [`Self::cancel_pending_request`] is called.
    #[doc(hidden)]
    async fn rpc_with_cleanup(
        &mut self,
        method: &str,
        params: &str,
        _cleanup: RequestCleanup,
    ) -> RpcResult<String> {
        self.rpc(method, params).await
    }

    /// Finish and clean up a request whose owning future was cancelled while
    /// waiting for its JSON-RPC response.
    #[doc(hidden)]
    async fn cancel_pending_request(&mut self) -> RpcResult<()> {
        Ok(())
    }
}

/// Backends that support JSON-RPC subscriptions (WebSocket, smoldot).
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
#[allow(async_fn_in_trait)]
pub trait RpcSubscription: Rpc {
    /// Subscribe to a method. Returns the subscription ID.
    async fn subscribe(&mut self, method: &str, params: &str) -> RpcResult<String>;

    /// Cancellation-aware subscription establishment.
    #[doc(hidden)]
    async fn subscribe_with_cleanup(
        &mut self,
        method: &str,
        params: &str,
        _cleanup: RequestCleanup,
    ) -> RpcResult<String> {
        self.subscribe(method, params).await
    }

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
#[cfg(all(feature = "ws-web", target_arch = "wasm32"))]
pub mod ws_web;

/// ChainHead v1 session manager.
#[cfg(any(
    feature = "ws",
    feature = "ws-edge",
    feature = "smoldot",
    all(feature = "ws-web", target_arch = "wasm32")
))]
pub mod chainhead;
#[cfg(all(feature = "smoldot", feature = "std"))]
pub mod managed_platform;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(feature = "ws", feature = "std"))]
    struct TrackedMock {
        tracker: RequestTracker,
        next_id: u32,
        events: alloc::collections::VecDeque<(String, String)>,
        responses: alloc::collections::VecDeque<IncomingMessage>,
        sent: Vec<String>,
        stall_send: bool,
    }

    #[cfg(all(feature = "ws", feature = "std"))]
    impl TrackedTransport for TrackedMock {
        fn request_tracker(&mut self) -> &mut RequestTracker {
            &mut self.tracker
        }

        fn next_request_id(&mut self) -> &mut u32 {
            &mut self.next_id
        }

        fn event_buffer(&mut self) -> &mut alloc::collections::VecDeque<(String, String)> {
            &mut self.events
        }

        async fn send_request_text(&mut self, request: &str) -> RpcResult<()> {
            self.sent.push(request.into());
            if request.contains("fail_send") {
                Err(JsonRpcError::new(-32603, "ambiguous send failure"))
            } else if self.stall_send {
                core::future::pending().await
            } else {
                Ok(())
            }
        }

        async fn read_tracked_message(&mut self) -> RpcResult<IncomingMessage> {
            match self.responses.pop_front() {
                Some(response) => Ok(response),
                None => core::future::pending().await,
            }
        }
    }

    #[cfg(all(feature = "ws", feature = "std"))]
    fn tracked_mock(stall_send: bool) -> TrackedMock {
        TrackedMock {
            tracker: RequestTracker::default(),
            next_id: 1,
            events: alloc::collections::VecDeque::new(),
            responses: alloc::collections::VecDeque::new(),
            sent: Vec::new(),
            stall_send,
        }
    }

    #[cfg(all(feature = "ws", feature = "std"))]
    #[test]
    fn cancelled_tracked_request_is_drained_and_cleaned() {
        smol::block_on(async {
            let mut transport = tracked_mock(false);
            let result = crate::time::timeout(
                core::time::Duration::from_millis(10),
                TrackedTransport::tracked_rpc_with_cleanup(
                    &mut transport,
                    "chainHead_v1_storage",
                    "[]",
                    RequestCleanup::StopChainHeadOperation {
                        follow_subscription: "follow".into(),
                    },
                ),
            )
            .await;
            assert!(result.is_err());
            assert_eq!(
                transport
                    .tracker
                    .pending_request
                    .as_ref()
                    .map(|request| request.id),
                Some(1)
            );

            transport
                .responses
                .push_back(IncomingMessage::Response(RpcResponse {
                    id: 1,
                    result: Some(
                        r#"{"result":"started","operationId":"abandoned","discardedItems":0}"#
                            .into(),
                    ),
                }));
            transport
                .responses
                .push_back(IncomingMessage::Response(RpcResponse {
                    id: 2,
                    result: Some("null".into()),
                }));
            TrackedTransport::reconcile_cleanup_retry(&mut transport)
                .await
                .unwrap();

            assert!(transport.tracker.pending_request.is_none());
            assert!(transport.tracker.pending_cleanup_retry.is_none());
            assert_eq!(transport.sent.len(), 2);
            assert!(transport.sent[1].contains("chainHead_v1_stopOperation"));
            assert!(transport.sent[1].contains("abandoned"));
        });
    }

    #[cfg(all(feature = "ws", feature = "std"))]
    #[test]
    fn abandoned_subscriptions_use_their_protocol_specific_cleanup() {
        smol::block_on(async {
            for (start_method, cleanup, cleanup_method) in [
                (
                    "chainHead_v1_follow",
                    RequestCleanup::UnfollowChainHead,
                    "chainHead_v1_unfollow",
                ),
                (
                    "archive_v1_storage",
                    RequestCleanup::StopArchiveStorage,
                    "archive_v1_stopStorage",
                ),
            ] {
                let mut transport = tracked_mock(false);
                let cancelled = crate::time::timeout(
                    core::time::Duration::from_millis(10),
                    TrackedTransport::tracked_rpc_with_cleanup(
                        &mut transport,
                        start_method,
                        "[]",
                        cleanup,
                    ),
                )
                .await;
                assert!(cancelled.is_err());

                transport
                    .responses
                    .push_back(IncomingMessage::Response(RpcResponse {
                        id: 1,
                        result: Some(r#""abandoned-subscription""#.into()),
                    }));
                transport
                    .responses
                    .push_back(IncomingMessage::Response(RpcResponse {
                        id: 2,
                        result: Some("null".into()),
                    }));
                TrackedTransport::reconcile_cleanup_retry(&mut transport)
                    .await
                    .unwrap();

                assert!(transport.tracker.pending_request.is_none());
                assert!(transport.tracker.pending_cleanup_retry.is_none());
                assert_eq!(transport.sent.len(), 2);
                assert!(transport.sent[1].contains(cleanup_method));
                assert!(transport.sent[1].contains("abandoned-subscription"));
            }
        });
    }

    #[cfg(all(feature = "ws", feature = "std"))]
    #[test]
    fn cancellation_during_send_retains_the_request_identity() {
        smol::block_on(async {
            let mut transport = tracked_mock(true);
            let result = crate::time::timeout(
                core::time::Duration::from_millis(10),
                TrackedTransport::tracked_rpc(&mut transport, "state_getMetadata", "[]"),
            )
            .await;
            assert!(result.is_err());
            assert_eq!(
                transport
                    .tracker
                    .pending_request
                    .as_ref()
                    .map(|request| request.id),
                Some(1)
            );
        });
    }

    #[cfg(all(feature = "ws", feature = "std"))]
    #[test]
    fn send_failure_retains_the_request_identity() {
        smol::block_on(async {
            let mut transport = tracked_mock(false);
            let error = TrackedTransport::tracked_rpc(&mut transport, "fail_send", "[]")
                .await
                .unwrap_err();
            assert_eq!(error.message, "ambiguous send failure");
            assert_eq!(
                transport
                    .tracker
                    .pending_request
                    .as_ref()
                    .map(|request| request.id),
                Some(1)
            );
        });
    }

    #[cfg(any(
        feature = "ws",
        feature = "ws-edge",
        feature = "smoldot",
        all(feature = "ws-web", target_arch = "wasm32")
    ))]
    #[test]
    fn to_hex_encodes_bytes() {
        assert_eq!(super::to_hex(&[0xde, 0xad]), "0xdead");
    }

    #[cfg(any(
        feature = "ws",
        feature = "ws-edge",
        feature = "smoldot",
        all(feature = "ws-web", target_arch = "wasm32")
    ))]
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
                assert_eq!(e.id, Some(1));
                assert_eq!(e.code, -32603);
                assert_eq!(e.message, "bad");
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn parse_response_with_whitespace_and_nested_content() {
        let json = r#"{ "result" : {"message":"contains \"error\"","items":[1,{"id":99}]}, "id" : 7, "jsonrpc" : "2.0" }"#;
        let msg = IncomingMessage::parse(json).unwrap();
        match msg {
            IncomingMessage::Response(response) => {
                assert_eq!(response.id, 7);
                assert_eq!(
                    response.result.as_deref(),
                    Some(r#"{"message":"contains \"error\"","items":[1,{"id":99}]}"#)
                );
            }
            _ => panic!("expected Response"),
        }
    }

    #[test]
    fn parse_notification_with_escaped_strings() {
        let json = r#"{"jsonrpc":"2.0","method":"chainHead_\u0076\u0031_event","params":{"subscription":"sub\u0031","result":{"event":"initialized"}}}"#;
        let msg = IncomingMessage::parse(json).unwrap();
        match msg {
            IncomingMessage::Notification(notification) => {
                assert_eq!(notification.method, "chainHead_v1_event");
                assert_eq!(notification.params.subscription, "sub1");
            }
            _ => panic!("expected Notification"),
        }
    }

    #[test]
    fn result_error_text_does_not_become_rpc_error() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"error":"domain value"}}"#;
        assert!(matches!(
            IncomingMessage::parse(json),
            Some(IncomingMessage::Response(_))
        ));
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(IncomingMessage::parse("not json at all").is_none());
        assert!(IncomingMessage::parse(r#"{"id":1,"result":[}"#).is_none());
    }

    #[cfg(any(
        feature = "ws",
        feature = "ws-edge",
        feature = "smoldot",
        all(feature = "ws-web", target_arch = "wasm32")
    ))]
    #[test]
    fn result_as_str_strips_quotes() {
        assert_eq!(result_as_str("\"hello\""), Some("hello"));
        assert_eq!(result_as_str("123"), None);
        assert_eq!(result_as_str("null"), None);
    }
}
