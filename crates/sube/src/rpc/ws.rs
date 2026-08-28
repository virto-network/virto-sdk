//! WebSocket transport — no spawning, inline message processing.

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};

use async_tungstenite::WebSocketStream;
use async_tungstenite::tungstenite::Message;
use async_tungstenite::tungstenite::client::IntoClientRequest;
use futures_util::StreamExt;

use super::{
    IncomingMessage, JsonRpcError, RequestCleanup, RequestTracker, Rpc, RpcResult, TrackedTransport,
};
use crate::Error;

#[cfg(feature = "wss")]
type WsInner = async_tungstenite::smol::ClientStream<smol::net::TcpStream>;
#[cfg(not(feature = "wss"))]
type WsInner = smol::net::TcpStream;

pub struct Backend {
    ws: WebSocketStream<WsInner>,
    event_buffer: VecDeque<(String, String)>,
    next_id: u32,
    request_tracker: RequestTracker,
}

impl Backend {
    pub async fn new(url: &str) -> Result<Self, Error> {
        log::trace!("WS connecting to {}", url);

        let mut request = url
            .into_client_request()
            .map_err(|e| Error::Node(format!("websocket request: {e}")))?;
        request.headers_mut().insert(
            "User-Agent",
            concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"))
                .parse()
                .expect("package name and version form a valid header"),
        );

        let host = request.uri().host().unwrap_or("localhost").to_string();
        let scheme = request.uri().scheme_str().unwrap_or("ws");
        let port = request
            .uri()
            .port_u16()
            .unwrap_or(if scheme == "wss" || scheme == "https" {
                443
            } else {
                80
            });

        let tcp = smol::net::TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|e| Error::Node(format!("tcp connect: {e}")))?;

        #[cfg(feature = "wss")]
        let (ws, _) = async_tungstenite::smol::client_async_tls(request, tcp)
            .await
            .map_err(|e| Error::Node(format!("websocket connect: {e}")))?;
        #[cfg(not(feature = "wss"))]
        let (ws, _) = async_tungstenite::client_async(request, tcp)
            .await
            .map_err(|e| Error::Node(format!("websocket connect: {e}")))?;

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
                    log::trace!("WS message: {}", &text);
                    if let Some(msg) = IncomingMessage::parse(&text) {
                        return Ok(msg);
                    }
                }
                Message::Close(_) => {
                    return Err(JsonRpcError::new(-32603, "connection closed by server"));
                }
                _ => {} // ping/pong handled by tungstenite
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
            .send(Message::Text(request.to_string().into()))
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
