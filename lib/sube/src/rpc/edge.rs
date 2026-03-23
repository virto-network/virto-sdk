//! WebSocket transport for embedded targets using `edge-ws`.
//!
//! Provides a JSON-RPC transport over WebSocket using the lightweight
//! `edge-ws` crate, which works on any `embedded_io_async::{Read, Write}` stream.
//! This enables sube to run on ESP32, embassy, and other no_std targets.
//!
//! ```rust,ignore
//! // Connect a TCP (or TLS) stream, then upgrade to WebSocket:
//! let ws = edge::Backend::connect(tcp_stream, "kreivo.io", "/").await?;
//! let mut chain = ChainHead::new(ws).await?;
//! let meta = chain.metadata().await?;
//! ```

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write as FmtWrite;

use edge_ws::{FrameHeader, FrameType};
use embedded_io_async::{Read, Write};

use super::{IncomingMessage, JsonRpcError, JsonRpcRequest, Rpc, RpcResult};
use crate::Error;

/// WebSocket backend over any `embedded_io_async` byte stream.
///
/// Use [`Backend::connect`] to perform the WebSocket upgrade handshake
/// on a TCP (or TLS) stream, or [`Backend::from_upgraded`] if the
/// handshake has already been done externally.
pub struct Backend<T> {
    stream: T,
    event_buffer: VecDeque<(String, serde_json::Value)>,
    next_id: u32,
    frag_buf: Vec<u8>,
}

impl<T: Read + Write> Backend<T> {
    /// Perform WebSocket upgrade handshake and return a ready backend.
    ///
    /// `stream` must be an already-connected TCP (or TLS) stream.
    /// `host` is used for the `Host` header, `path` for the request URI.
    pub async fn connect(mut stream: T, host: &str, path: &str) -> Result<Self, Error> {
        ws_handshake(&mut stream, host, path).await?;
        Ok(Self::from_upgraded(stream))
    }

    /// Wrap a stream where the WebSocket handshake has already completed.
    pub fn from_upgraded(stream: T) -> Self {
        Backend {
            stream,
            event_buffer: VecDeque::new(),
            next_id: 1,
            frag_buf: Vec::new(),
        }
    }

    /// Send a complete text frame (client-masked per RFC 6455).
    async fn send_text(&mut self, payload: &[u8]) -> Result<(), JsonRpcError> {
        let header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: payload.len() as u64,
            mask_key: Some(self.next_id),
        };
        header
            .send(&mut self.stream)
            .await
            .map_err(|e| ws_err("send header", e))?;
        header
            .send_payload(&mut self.stream, payload)
            .await
            .map_err(|e| ws_err("send payload", e))
    }

    /// Read a single WebSocket frame, allocating exactly the needed buffer.
    async fn read_frame(&mut self) -> Result<(FrameType, Vec<u8>), JsonRpcError> {
        let header = FrameHeader::recv(&mut self.stream)
            .await
            .map_err(|e| ws_err("recv header", e))?;
        let len = header.payload_len as usize;
        let mut buf = vec![0u8; len];
        header
            .recv_payload(&mut self.stream, &mut buf)
            .await
            .map_err(|e| ws_err("recv payload", e))?;
        Ok((header.frame_type, buf))
    }

    /// Read frames until a complete JSON-RPC message is assembled.
    /// Handles fragmentation, ping/pong, and close frames.
    async fn read_message(&mut self) -> Result<IncomingMessage, JsonRpcError> {
        loop {
            let (frame_type, data) = self.read_frame().await?;
            match frame_type {
                FrameType::Text(false) => {
                    let text = core::str::from_utf8(&data)
                        .map_err(|_| JsonRpcError::new(-32603, "invalid utf8"))?;
                    log::trace!("WS recv: {}", text);
                    if let Some(msg) = IncomingMessage::parse(text) {
                        return Ok(msg);
                    }
                }
                FrameType::Text(true) => {
                    self.frag_buf.clear();
                    self.frag_buf.extend_from_slice(&data);
                }
                FrameType::Continue(true) => {
                    self.frag_buf.extend_from_slice(&data);
                    let text = core::str::from_utf8(&self.frag_buf)
                        .map_err(|_| JsonRpcError::new(-32603, "invalid utf8"))?;
                    log::trace!("WS recv (assembled): {}", text);
                    if let Some(msg) = IncomingMessage::parse(text) {
                        return Ok(msg);
                    }
                }
                FrameType::Continue(false) => {
                    self.frag_buf.extend_from_slice(&data);
                }
                FrameType::Ping => {
                    let pong = FrameHeader {
                        frame_type: FrameType::Pong,
                        payload_len: data.len() as u64,
                        mask_key: Some(self.next_id),
                    };
                    let _ = pong.send(&mut self.stream).await;
                    let _ = pong.send_payload(&mut self.stream, &data).await;
                }
                FrameType::Close => {
                    return Err(JsonRpcError::new(-32603, "connection closed by server"));
                }
                _ => {} // Pong, Binary — ignore
            }
        }
    }
}

impl<T: Read + Write> Rpc for Backend<T> {
    async fn rpc(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> RpcResult<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        log::info!("RPC `{}` (ID={})", method, id);

        let msg = serde_json::to_vec(&JsonRpcRequest {
            id,
            jsonrpc: "2.0",
            method,
            params: Some(params),
        })
        .map_err(|e| JsonRpcError::new(-32603, &alloc::format!("serialize: {e}")))?;

        self.send_text(&msg).await?;

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

impl<T: Read + Write> super::RpcSubscription for Backend<T> {
    async fn subscribe(&mut self, method: &str, params: serde_json::Value) -> RpcResult<String> {
        let sub_id: String = serde_json::from_value(self.rpc(method, params).await?)
            .map_err(|e| JsonRpcError::new(-32603, &alloc::format!("bad sub id: {e}")))?;
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

/// Map an edge-ws error to a JsonRpcError, discarding the inner IO error type.
fn ws_err<E>(ctx: &str, e: edge_ws::Error<E>) -> JsonRpcError {
    let detail = match e {
        edge_ws::Error::Incomplete(n) => alloc::format!("{ctx}: incomplete ({n} bytes short)"),
        edge_ws::Error::Invalid => alloc::format!("{ctx}: invalid frame"),
        edge_ws::Error::BufferOverflow => alloc::format!("{ctx}: buffer overflow"),
        edge_ws::Error::InvalidLen => alloc::format!("{ctx}: invalid length"),
        edge_ws::Error::Io(_) => alloc::format!("{ctx}: io error"),
    };
    JsonRpcError::new(-32603, &detail)
}

/// Minimal WebSocket upgrade handshake (RFC 6455 §4.1).
///
/// Sends an HTTP/1.1 Upgrade request and waits for 101 Switching Protocols.
/// Uses a fixed Sec-WebSocket-Key — the key is for proxy cache busting,
/// not security, and substrate nodes don't validate the accept hash.
async fn ws_handshake(
    stream: &mut (impl Read + Write),
    host: &str,
    path: &str,
) -> Result<(), Error> {
    let mut request = String::with_capacity(256);
    let _ = write!(request, "GET {path} HTTP/1.1\r\n");
    let _ = write!(request, "Host: {host}\r\n");
    request.push_str("Upgrade: websocket\r\n");
    request.push_str("Connection: Upgrade\r\n");
    request.push_str("Sec-WebSocket-Key: c3ViZS1lbWJlZGRlZC1rZXk=\r\n");
    request.push_str("Sec-WebSocket-Version: 13\r\n");
    request.push_str("User-Agent: sube/1.0\r\n");
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|_| Error::Node("ws handshake write failed".into()))?;

    let mut buf = [0u8; 1024];
    let mut total = 0;
    loop {
        let n = stream
            .read(&mut buf[total..])
            .await
            .map_err(|_| Error::Node("ws handshake read failed".into()))?;
        if n == 0 {
            return Err(Error::Node("connection closed during handshake".into()));
        }
        total += n;

        if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
            let status_end = buf[..total]
                .windows(2)
                .position(|w| w == b"\r\n")
                .unwrap_or(total);
            let status_line = core::str::from_utf8(&buf[..status_end]).unwrap_or("");
            if !status_line.contains("101") {
                return Err(Error::Node(alloc::format!(
                    "ws upgrade rejected: {status_line}"
                )));
            }
            return Ok(());
        }
        if total >= buf.len() {
            return Err(Error::Node("handshake response too large".into()));
        }
    }
}
