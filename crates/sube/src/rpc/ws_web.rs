//! Browser WebSocket transport — uses the native WebSocket via gloo-net.
//!
//! Target: `wasm32-unknown-unknown` in a browser context. The browser
//! handles TCP, TLS (`wss://`) and framing; sube only drives JSON-RPC
//! over text messages.

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};

use futures_util::{SinkExt, StreamExt};
use gloo_net::websocket::{Message, futures::WebSocket};

use super::{
    IncomingMessage, JsonRpcError, RequestCleanup, RequestTracker, Rpc, RpcResult, TrackedTransport,
};
use crate::Error;

pub struct Backend {
    ws: WebSocket,
    event_buffer: VecDeque<(String, String)>,
    next_id: u32,
    request_tracker: RequestTracker,
}

impl Backend {
    pub async fn new(url: &str) -> Result<Self, Error> {
        log::trace!("WS(web) connecting to {}", url);
        let ws = WebSocket::open(url).map_err(|e| Error::Node(format!("websocket open: {e}")))?;
        Ok(Backend {
            ws,
            event_buffer: VecDeque::new(),
            next_id: 1,
            request_tracker: RequestTracker::default(),
        })
    }

    /// Read the next meaningful message from the WebSocket.
    async fn read_message(&mut self) -> Result<IncomingMessage, JsonRpcError> {
        loop {
            let frame = self
                .ws
                .next()
                .await
                .ok_or_else(|| JsonRpcError::new(-32603, "connection closed"))?
                .map_err(|e| JsonRpcError::new(-32603, &format!("ws read: {e}")))?;
            match frame {
                Message::Text(text) => {
                    log::trace!("WS(web) message: {}", &text);
                    if let Some(msg) = IncomingMessage::parse(&text) {
                        return Ok(msg);
                    }
                }
                // Binary frames are not used by Substrate JSON-RPC; ignore.
                Message::Bytes(_) => {}
            }
        }
    }
}

impl TrackedTransport for Backend {
    fn request_tracker(&mut self) -> &mut RequestTracker {
        &mut self.request_tracker
    }

    fn next_request_id(&mut self) -> &mut u32 {
        &mut self.next_id
    }

    fn event_buffer(&mut self) -> &mut VecDeque<(String, String)> {
        &mut self.event_buffer
    }

    async fn send_request_text(&mut self, request: &str) -> RpcResult<()> {
        self.ws
            .send(Message::Text(request.to_string()))
            .await
            .map_err(|error| JsonRpcError::new(-32603, &format!("ws send: {error}")))
    }

    async fn read_tracked_message(&mut self) -> RpcResult<IncomingMessage> {
        self.read_message().await
    }
}

impl super::Rpc for Backend {
    async fn rpc(&mut self, method: &str, params: &str) -> RpcResult<String> {
        TrackedTransport::tracked_rpc(self, method, params).await
    }

    async fn rpc_with_cleanup(
        &mut self,
        method: &str,
        params: &str,
        cleanup: RequestCleanup,
    ) -> RpcResult<String> {
        TrackedTransport::tracked_rpc_with_cleanup(self, method, params, cleanup).await
    }

    async fn cancel_pending_request(&mut self) -> RpcResult<()> {
        TrackedTransport::reconcile_cleanup_retry(self).await
    }
}

impl super::RpcSubscription for Backend {
    async fn subscribe(&mut self, method: &str, params: &str) -> RpcResult<String> {
        let result = self.rpc(method, params).await?;
        super::result_as_str(&result)
            .map(|s| s.to_string())
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
            .map(ToString::to_string)
            .ok_or_else(|| JsonRpcError::new(-32603, "expected string subscription id"))
    }

    async fn next_event(&mut self) -> Option<(String, String)> {
        if TrackedTransport::reconcile_cleanup_retry(self)
            .await
            .is_err()
        {
            return None;
        }
        if let Some(event) = self.event_buffer.pop_front() {
            return Some(event);
        }
        loop {
            match self.read_message().await {
                Ok(IncomingMessage::Notification(n)) => {
                    return Some((n.params.subscription, n.params.result));
                }
                Ok(IncomingMessage::Response(_)) => {}
                Ok(IncomingMessage::Error(e)) => {
                    log::warn!("rpc error while waiting for event: {e}");
                }
                Err(e) => {
                    log::warn!("ws error while waiting for event: {e}");
                    return None;
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
