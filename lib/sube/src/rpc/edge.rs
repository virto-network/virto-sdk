//! WebSocket transport for embedded targets using `edge-ws`.
//!
//! Provides a JSON-RPC transport over WebSocket using the lightweight
//! `edge-ws` crate, which works on any `embedded_io_async::{Read, Write}` stream.
//! This enables sube to run on ESP32, embassy, and other no_std targets.
//!
//! Implement [`Connect`] to provide your platform's TCP/TLS transport,
//! then use [`Sube::connect_edge`](crate::Sube::connect_edge) for a
//! one-liner connection:
//!
//! ```rust,ignore
//! let mut chain = Sube::connect_edge("wss://kreivo.io", &mut tls, &["Balances"]).await?;
//! let r = chain.query("balances/total-issuance").await?;
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
        if len == 0 {
            return Ok((header.frame_type, Vec::new()));
        }
        if len > 512 * 1024 {
            return Err(JsonRpcError::new(
                -32603,
                "frame too large for embedded target",
            ));
        }
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

// --- Streaming hex reader ---

/// Reads a large WebSocket message, hex-decoding the `"output":"0x..."` field
/// on the fly in small chunks without allocating the full frame.
/// Implements `embedded_io_async::Read` so it can feed a `StreamCursor`.
pub struct HexFrameReader<'a, T> {
    backend: &'a mut Backend<T>,
    /// Decoded bytes ready to be read.
    decoded: Vec<u8>,
    /// Read position within `decoded`.
    read_pos: usize,
    /// True once we've found the "0x" marker and are decoding hex.
    in_hex: bool,
    /// Carry byte from an odd-length hex chunk.
    carry: Option<u8>,
    /// Remaining payload bytes in the current frame.
    remaining: usize,
    /// True if current frame is the last (or only) one.
    is_final: bool,
    /// True once hex stream is complete.
    done: bool,
}

impl<'a, T: Read + Write> HexFrameReader<'a, T> {
    pub fn new(backend: &'a mut Backend<T>) -> Self {
        Self {
            backend,
            decoded: Vec::new(),
            read_pos: 0,
            in_hex: false,
            carry: None,
            remaining: 0,
            is_final: false,
            done: false,
        }
    }

    /// Read the next chunk from the current frame, or start a new frame.
    async fn fetch_next_chunk(&mut self) -> Result<bool, Error> {
        if self.done {
            return Ok(false);
        }

        loop {
            // If we have remaining payload in the current frame, read a chunk
            if self.remaining > 0 {
                let mut chunk = [0u8; 1024];
                let to_read = self.remaining.min(chunk.len());
                read_exact(&mut self.backend.stream, &mut chunk[..to_read])
                    .await
                    .map_err(|_| Error::Node("frame payload read failed".into()))?;
                self.remaining -= to_read;

                self.decoded.clear();
                self.read_pos = 0;

                if !self.in_hex {
                    if let Some(pos) = find_hex_start(&chunk[..to_read]) {
                        self.in_hex = true;
                        log::debug!("hex reader: found hex start at offset {}", pos);
                        self.decode_hex_chunk(&chunk[pos..to_read]);
                    }
                } else {
                    self.decode_hex_chunk(&chunk[..to_read]);
                }

                // Frame fully consumed — check if message is complete
                if self.remaining == 0 && self.is_final {
                    self.done = true;
                }

                if !self.decoded.is_empty() {
                    return Ok(true);
                }

                if self.done {
                    return Ok(false);
                }
                continue;
            }

            // Start a new frame
            let header = FrameHeader::recv(&mut self.backend.stream)
                .await
                .map_err(|_| Error::Node("hex reader: frame header read failed".into()))?;

            match header.frame_type {
                FrameType::Text(fragmented) => {
                    self.remaining = header.payload_len as usize;
                    self.is_final = !fragmented;
                }
                FrameType::Continue(fin) => {
                    self.remaining = header.payload_len as usize;
                    self.is_final = fin;
                }
                FrameType::Ping => {
                    // Read ping payload and pong
                    let len = (header.payload_len as usize).min(125);
                    let mut ping = [0u8; 125];
                    if len > 0 {
                        read_exact(&mut self.backend.stream, &mut ping[..len])
                            .await
                            .ok();
                    }
                    let pong = FrameHeader {
                        frame_type: FrameType::Pong,
                        payload_len: len as u64,
                        mask_key: Some(0),
                    };
                    let _ = pong.send(&mut self.backend.stream).await;
                    let _ = pong
                        .send_payload(&mut self.backend.stream, &ping[..len])
                        .await;
                    continue;
                }
                FrameType::Close => {
                    self.done = true;
                    return Err(Error::Node("connection closed during hex stream".into()));
                }
                _ => {
                    // Skip unknown frame payload
                    let mut skip = header.payload_len as usize;
                    let mut discard = [0u8; 256];
                    while skip > 0 {
                        let n = skip.min(discard.len());
                        let _ = read_exact(&mut self.backend.stream, &mut discard[..n]).await;
                        skip -= n;
                    }
                    continue;
                }
            }

            // For small complete frames without hex, skip (block events etc.)
            if self.remaining < 4096 && self.is_final && !self.in_hex {
                let len = self.remaining;
                let mut small = Vec::new();
                small.resize(len, 0);
                read_exact(&mut self.backend.stream, &mut small)
                    .await
                    .map_err(|_| Error::Node("small frame read failed".into()))?;
                self.remaining = 0;

                if find_hex_start(&small).is_some() {
                    // Oops, it does have hex — process it
                    let pos = find_hex_start(&small).unwrap();
                    self.in_hex = true;
                    self.decoded.clear();
                    self.read_pos = 0;
                    self.decode_hex_chunk(&small[pos..]);
                    self.done = true; // single frame = done
                    if !self.decoded.is_empty() {
                        return Ok(true);
                    }
                    return Ok(false);
                }
                log::debug!("hex reader: skipped small frame ({} bytes)", len);
                continue;
            }
        }
    }

    /// Hex-decode a chunk of text, handling the closing `"` and carry byte.
    fn decode_hex_chunk(&mut self, data: &[u8]) {
        let mut i = 0;

        if let Some(hi) = self.carry.take() {
            if i < data.len() {
                if data[i] == b'"' {
                    self.done = true;
                    return;
                }
                if let (Some(h), Some(l)) = (hex_val(hi), hex_val(data[i])) {
                    self.decoded.push((h << 4) | l);
                }
                i += 1;
            }
        }

        while i < data.len() {
            if data[i] == b'"' {
                self.done = true;
                return;
            }
            if i + 1 >= data.len() {
                self.carry = Some(data[i]);
                return;
            }
            if let (Some(h), Some(l)) = (hex_val(data[i]), hex_val(data[i + 1])) {
                self.decoded.push((h << 4) | l);
            }
            i += 2;
        }
    }
}

impl<T: Read + Write> embedded_io_async::ErrorType for HexFrameReader<'_, T> {
    type Error = embedded_io_async::ErrorKind;
}

impl<T: Read + Write> embedded_io_async::Read for HexFrameReader<'_, T> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let available = self.decoded.len() - self.read_pos;
        if available > 0 {
            let n = available.min(buf.len());
            buf[..n].copy_from_slice(&self.decoded[self.read_pos..self.read_pos + n]);
            self.read_pos += n;
            return Ok(n);
        }

        match self.fetch_next_chunk().await {
            Ok(true) => {
                let n = self.decoded.len().min(buf.len());
                buf[..n].copy_from_slice(&self.decoded[..n]);
                self.read_pos = n;
                Ok(n)
            }
            Ok(false) => Ok(0),
            Err(e) => {
                log::error!("hex reader: {e}");
                Err(embedded_io_async::ErrorKind::Other)
            }
        }
    }
}

/// Read exactly `buf.len()` bytes from the stream.
async fn read_exact(stream: &mut (impl Read + Write), buf: &mut [u8]) -> Result<(), ()> {
    let mut pos = 0;
    while pos < buf.len() {
        match stream.read(&mut buf[pos..]).await {
            Ok(0) => return Err(()),
            Ok(n) => pos += n,
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Find the start of hex data after `"0x` in a JSON text chunk.
fn find_hex_start(data: &[u8]) -> Option<usize> {
    // Look for "0x pattern (quote, 0, x)
    for i in 0..data.len().saturating_sub(2) {
        if data[i] == b'"' && data[i + 1] == b'0' && data[i + 2] == b'x' {
            return Some(i + 3); // start after "0x
        }
    }
    None
}

/// Parsed components of a `ws://` or `wss://` URL.
struct WsUrl<'a> {
    host: &'a str,
    port: u16,
    path: &'a str,
    tls: bool,
}

/// Parse a WebSocket URL into host, port, path, and TLS flag.
///
/// Supports `ws://`, `wss://`, or bare `host:port/path` (defaults to wss).
fn parse_url(url: &str) -> crate::Result<WsUrl<'_>> {
    let (tls, rest) = if let Some(r) = url.strip_prefix("wss://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("ws://") {
        (false, r)
    } else {
        (true, url)
    };

    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };

    let (host, port) = match host_port.rfind(':') {
        Some(i) => {
            let p = host_port[i + 1..]
                .parse::<u16>()
                .map_err(|_| Error::BadInput)?;
            (&host_port[..i], p)
        }
        None => (host_port, if tls { 443 } else { 80 }),
    };

    Ok(WsUrl {
        host,
        port,
        path,
        tls,
    })
}

type TcpSocket = embassy_net::tcp::TcpSocket<'static>;
type TlsSession = mbedtls_rs::Session<'static, TcpSocket>;

/// Socket type for edge connections — plain TCP or TLS.
pub enum EdgeSocket {
    Tcp(TcpSocket),
    Tls(TlsSession),
}

impl embedded_io_async::ErrorType for EdgeSocket {
    type Error = embedded_io_async::ErrorKind;
}

impl Read for EdgeSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Tcp(s) => Read::read(s, buf)
                .await
                .map_err(|_| embedded_io_async::ErrorKind::Other),
            Self::Tls(s) => Read::read(s, buf)
                .await
                .map_err(|_| embedded_io_async::ErrorKind::Other),
        }
    }
}

impl Write for EdgeSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Tcp(s) => Write::write(s, buf)
                .await
                .map_err(|_| embedded_io_async::ErrorKind::Other),
            Self::Tls(s) => Write::write(s, buf)
                .await
                .map_err(|_| embedded_io_async::ErrorKind::Other),
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Tcp(s) => Write::flush(s)
                .await
                .map_err(|_| embedded_io_async::ErrorKind::Other),
            Self::Tls(s) => Write::flush(s)
                .await
                .map_err(|_| embedded_io_async::ErrorKind::Other),
        }
    }
}

/// Establish a WebSocket connection from a URL and network stack.
///
/// For `wss://` URLs: DNS → TCP → TLS → WebSocket.
/// For `ws://` URLs: DNS → TCP → WebSocket (no TLS, for local testing).
///
/// Socket buffers and TLS state are heap-allocated with `'static` lifetime,
/// suitable for single-connection embedded devices.
/// Network context for embedded edge connections.
///
/// Groups the embassy network stack, socket buffers, and provides
/// everything [`connect_edge`](crate::connect_edge) needs besides the URL.
/// Store in a `static` and initialize once at startup.
///
/// ```rust,ignore
/// static NET: sube::EdgeNet = sube::EdgeNet::new();
/// // after WiFi is up:
/// NET.init(stack, rng);
/// let mut chain = sube::connect_edge("wss://kreivo.io", &NET, &["CollatorSelection"]).await?;
/// ```
pub struct EdgeNet {
    stack: core::cell::UnsafeCell<Option<embassy_net::Stack<'static>>>,
    rx: core::cell::UnsafeCell<[u8; 2048]>,
    tx: core::cell::UnsafeCell<[u8; 2048]>,
}

// SAFETY: single-threaded embassy executor, only one connection at a time.
unsafe impl Sync for EdgeNet {}

impl EdgeNet {
    pub const fn new() -> Self {
        Self {
            stack: core::cell::UnsafeCell::new(None),
            rx: core::cell::UnsafeCell::new([0u8; 2048]),
            tx: core::cell::UnsafeCell::new([0u8; 2048]),
        }
    }

    /// Set the network stack. Call once after WiFi is connected.
    pub fn init(&self, stack: embassy_net::Stack<'static>) {
        // SAFETY: single-threaded, called once at startup.
        unsafe { *self.stack.get() = Some(stack) };
    }

    fn stack(&self) -> crate::Result<embassy_net::Stack<'static>> {
        // SAFETY: single-threaded, init called before use.
        unsafe { (*self.stack.get()).ok_or(crate::Error::ChainUnavailable) }
    }
}

/// Establish a WebSocket connection from a URL and network stack.
///
/// For `wss://` URLs: DNS → TCP → TLS → WebSocket.
/// For `ws://` URLs: DNS → TCP → WebSocket (no TLS, for local testing).
///
/// `bufs` must have `'static` lifetime — use a `StaticCell` on embedded.
/// TLS state is heap-allocated on first call. The `Tls` singleton is
/// reused if this function is called again (mbedtls drops the old one).
pub async fn edge_connect<R: rand_core::CryptoRng + Send + 'static>(
    url: &str,
    net: &'static EdgeNet,
    rng: R,
) -> crate::Result<Backend<EdgeSocket>> {
    use alloc::boxed::Box;
    use alloc::format;

    let parsed = parse_url(url)?;
    let stack = net.stack()?;
    log::info!("edge: connecting to {} (tls={})", parsed.host, parsed.tls);

    // SAFETY: single-threaded executor, only one connection at a time.
    let (rx, tx) = unsafe { (&mut *net.rx.get(), &mut *net.tx.get()) };
    let mut socket = TcpSocket::new(stack, rx, tx);
    socket.set_timeout(Some(embassy_time::Duration::from_secs(15)));

    // DNS
    log::debug!("edge: DNS lookup for {}", parsed.host);
    let remote = stack
        .dns_query(parsed.host, embassy_net::dns::DnsQueryType::A)
        .await
        .map_err(|e| {
            log::error!("DNS failed for {}: {:?}", parsed.host, e);
            Error::Node(format!("DNS failed for {}", parsed.host))
        })?[0];
    log::debug!("edge: DNS resolved to {:?}", remote);

    // TCP
    socket.connect((remote, parsed.port)).await.map_err(|e| {
        log::error!(
            "TCP connect to {}:{} failed: {:?}",
            parsed.host,
            parsed.port,
            e
        );
        Error::Node("TCP connect failed".into())
    })?;
    log::info!("edge: TCP connected to {}:{}", parsed.host, parsed.port);

    let stream = if parsed.tls {
        log::debug!("edge: starting TLS handshake");

        // Drop the previous Tls singleton (if any) so we can create a new one.
        // SAFETY: single-threaded, the old Session referencing it has been dropped.
        use core::sync::atomic::{AtomicPtr, Ordering};
        static TLS_PTR: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
        let old = TLS_PTR.swap(core::ptr::null_mut(), Ordering::Relaxed);
        if !old.is_null() {
            unsafe { drop(Box::from_raw(old as *mut mbedtls_rs::Tls<'static>)) };
        }

        let rng = Box::leak(Box::new(rng));
        let tls_ctx =
            mbedtls_rs::Tls::new(rng).map_err(|e| Error::Node(format!("TLS init: {e:?}")))?;
        let tls_ctx = Box::leak(Box::new(tls_ctx));

        TLS_PTR.store(tls_ctx as *mut _ as *mut u8, Ordering::Relaxed);
        let host_cstr = Box::leak(format!("{}\0", parsed.host).into_boxed_str());
        let conf = Box::leak(Box::new(mbedtls_rs::SessionConfig::Client(
            mbedtls_rs::ClientSessionConfig {
                server_name: Some(
                    core::ffi::CStr::from_bytes_with_nul(host_cstr.as_bytes()).unwrap(),
                ),
                auth_mode: mbedtls_rs::AuthMode::None,
                ..mbedtls_rs::ClientSessionConfig::new()
            },
        )));
        let mut session = mbedtls_rs::Session::new(tls_ctx.reference(), socket, conf)
            .map_err(|e| Error::Node(format!("TLS session: {e:?}")))?;
        session
            .connect()
            .await
            .map_err(|e| Error::Node(format!("TLS handshake: {e:?}")))?;
        log::info!("TLS connected");
        EdgeSocket::Tls(session)
    } else {
        log::info!("plain TCP (no TLS)");
        EdgeSocket::Tcp(socket)
    };

    // WebSocket upgrade
    log::debug!("edge: WebSocket upgrade to {}{}", parsed.host, parsed.path);
    let backend = Backend::connect(stream, parsed.host, parsed.path).await?;
    log::info!("edge: WebSocket connected");
    Ok(backend)
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
