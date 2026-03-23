//! WebSocket transport — no spawning, inline message processing.

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};

use async_tungstenite::tungstenite::client::IntoClientRequest;
use async_tungstenite::tungstenite::Message;
use async_tungstenite::WebSocketStream;
use futures_util::StreamExt;

use super::{IncomingMessage, JsonRpcError, JsonRpcRequest, Rpc, RpcResult};
use crate::Error;

#[cfg(feature = "wss")]
type WsInner = async_tungstenite::stream::Stream<
    smol::net::TcpStream,
    async_tls::client::TlsStream<smol::net::TcpStream>,
>;
#[cfg(not(feature = "wss"))]
type WsInner = smol::net::TcpStream;

pub struct Backend {
    ws: WebSocketStream<WsInner>,
    event_buffer: VecDeque<(String, serde_json::Value)>,
    next_id: u32,
}

impl Backend {
    pub async fn new(url: &str) -> Result<Self, Error> {
        log::trace!("WS connecting to {}", url);

        let mut request = url
            .into_client_request()
            .map_err(|e| Error::Node(format!("websocket request: {e}")))?;
        request
            .headers_mut()
            .insert("User-Agent", "sube/1.0".parse().expect("valid header"));

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
        let (ws, _) = async_tungstenite::async_tls::client_async_tls(request, tcp)
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

impl super::Rpc for Backend {
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

        self.ws
            .send(Message::Text(msg.into()))
            .await
            .map_err(|e| JsonRpcError::new(-32603, &format!("ws send: {e}")))?;

        loop {
            match self.read_message().await? {
                IncomingMessage::Response(r)
                    if r.id.as_ref().and_then(|v| v.as_u64()) == Some(id as u64) =>
                {
                    return r.into_result();
                }
                IncomingMessage::Response(r) => {
                    log::warn!("unexpected response id: {:?}", r.id);
                }
                IncomingMessage::Notification(n) => {
                    self.event_buffer
                        .push_back((n.params.subscription, n.params.result));
                }
            }
        }
    }
}

impl super::RpcSubscription for Backend {
    async fn subscribe(&mut self, method: &str, params: serde_json::Value) -> RpcResult<String> {
        let sub_id: String = serde_json::from_value(self.rpc(method, params).await?)
            .map_err(|e| JsonRpcError::new(-32603, &format!("bad sub id: {e}")))?;
        Ok(sub_id)
    }

    async fn next_event(&mut self) -> Option<(String, serde_json::Value)> {
        if let Some(event) = self.event_buffer.pop_front() {
            return Some(event);
        }
        loop {
            match self.read_message().await {
                Ok(IncomingMessage::Notification(n)) => {
                    return Some((n.params.subscription, n.params.result))
                }
                Ok(IncomingMessage::Response(r)) => {
                    log::warn!("unexpected response while waiting for event: {:?}", r.id);
                }
                Err(e) => {
                    log::warn!("ws error while waiting for event: {e}");
                    return None;
                }
            }
        }
    }

    fn try_next_event(&mut self) -> Option<(String, serde_json::Value)> {
        self.event_buffer.pop_front()
    }

    async fn unsubscribe(&mut self, method: &str, sub_id: &str) -> RpcResult<()> {
        let _ = self.rpc(method, serde_json::json!([sub_id])).await?;
        Ok(())
    }
}
