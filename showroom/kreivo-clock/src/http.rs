//! Tiny HTTP server for device configuration.
//!
//! Serves a config page at `GET /` and accepts metadata uploads at `POST /metadata`.
//! Runs as an embassy task on core 0, sharing the network stack with the chain connection.

use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::fmt::Write as FmtWrite;

use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;

use crate::{BATTERY_LEVEL, WIFI_CONNECTED};

/// Holds raw metadata bytes pushed via HTTP, consumed by the main loop.
///
/// SAFETY: single-threaded — both http_task and watch_chain run on
/// core 0's embassy executor. Same pattern as `EdgeNet`.
pub struct MetadataSlot {
    buf: UnsafeCell<Option<Vec<u8>>>,
}

unsafe impl Sync for MetadataSlot {}

impl MetadataSlot {
    pub const fn new() -> Self {
        Self {
            buf: UnsafeCell::new(None),
        }
    }

    /// Store raw SCALE metadata bytes (called by http_task).
    pub fn store(&self, data: Vec<u8>) {
        unsafe { *self.buf.get() = Some(data) };
    }

    /// Take raw bytes if available (called by main loop).
    pub fn take(&self) -> Option<Vec<u8>> {
        unsafe { (*self.buf.get()).take() }
    }
}

pub static METADATA_SLOT: MetadataSlot = MetadataSlot::new();

/// Current block number, set by main loop, read by HTTP status endpoint.
pub static BLOCK_NUMBER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Number of loaded pallets (0 = no metadata).
pub static PALLET_COUNT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Set when metadata has been saved to flash.
pub static META_SAVED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

const HTML: &str = include_str!("../ui/config.html");

#[embassy_executor::task]
pub async fn http_task(stack: embassy_net::Stack<'static>) {
    static HTTP_RX: StaticCell<[u8; 2048]> = StaticCell::new();
    static HTTP_TX: StaticCell<[u8; 2048]> = StaticCell::new();
    let rx = HTTP_RX.init([0u8; 2048]);
    let tx = HTTP_TX.init([0u8; 2048]);

    loop {
        let mut socket = TcpSocket::new(stack, rx, tx);
        socket.set_timeout(Some(Duration::from_secs(30)));

        if let Err(e) = socket.accept(80).await {
            log::warn!("http: accept error: {:?}", e);
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }

        if let Err(e) = handle_connection(&mut socket).await {
            log::warn!("http: {}", e);
        }
        socket.close();
        Timer::after(Duration::from_millis(10)).await;
    }
}

async fn handle_connection(socket: &mut TcpSocket<'_>) -> Result<(), &'static str> {
    let mut buf = [0u8; 512];
    let mut total = 0;

    let header_end = loop {
        if total >= buf.len() {
            return Err("headers too large");
        }
        let n = socket.read(&mut buf[total..]).await.map_err(|_| "read error")?;
        if n == 0 {
            return Err("connection closed");
        }
        total += n;

        if let Some(pos) = buf[..total].windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
    };
    let headers = core::str::from_utf8(&buf[..header_end]).map_err(|_| "bad utf8")?;
    let body_start = &buf[header_end + 4..total];

    let first_line = headers.lines().next().ok_or("empty request")?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next().ok_or("no method")?;
    let path = parts.next().ok_or("no path")?;

    match (method, path) {
        ("GET", "/") => serve_html(socket).await,
        ("GET", "/status") => serve_status(socket).await,
        ("POST", "/metadata") => {
            let content_length = parse_content_length(headers).ok_or("missing Content-Length")?;
            receive_metadata(socket, content_length, body_start).await
        }
        ("OPTIONS", _) => send_cors_preflight(socket).await,
        _ => send_response(socket, 404, "text/plain", b"not found").await,
    }
}

fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        if line.len() > 15 && line[..15].eq_ignore_ascii_case("content-length:") {
            return line[15..].trim().parse().ok();
        }
    }
    None
}

async fn serve_html(socket: &mut TcpSocket<'_>) -> Result<(), &'static str> {
    let mut hdr = heapless::String::<256>::new();
    write!(
        hdr,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        HTML.len()
    )
    .ok();
    write_all(socket, hdr.as_bytes()).await?;

    for chunk in HTML.as_bytes().chunks(2048) {
        write_all(socket, chunk).await?;
    }
    Ok(())
}

async fn serve_status(socket: &mut TcpSocket<'_>) -> Result<(), &'static str> {
    use core::sync::atomic::Ordering::Relaxed;

    let block = BLOCK_NUMBER.load(Relaxed);
    let pallets = PALLET_COUNT.load(Relaxed);
    let wifi = WIFI_CONNECTED.load(Relaxed);
    let batt = BATTERY_LEVEL.load(Relaxed);
    let battery = if batt <= 100 { batt as i32 } else { -1 };

    let mut body = heapless::String::<128>::new();
    write!(
        body,
        r#"{{"block":{},"pallets":{},"wifi":{},"battery":{}}}"#,
        block, pallets, wifi, battery
    )
    .ok();

    let mut hdr = heapless::String::<256>::new();
    write!(
        hdr,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .ok();
    write_all(socket, hdr.as_bytes()).await?;
    write_all(socket, body.as_bytes()).await?;
    Ok(())
}

async fn receive_metadata(
    socket: &mut TcpSocket<'_>,
    content_length: usize,
    initial: &[u8],
) -> Result<(), &'static str> {
    if content_length > 512 * 1024 {
        return send_response(socket, 413, "text/plain", b"payload too large (max 512KB)").await;
    }

    log::info!("http: receiving metadata ({} bytes)", content_length);

    // Allocate in PSRAM (internal heap full → overflows to PSRAM)
    let mut buf = Vec::with_capacity(content_length);
    buf.extend_from_slice(initial);

    let mut chunk = [0u8; 4096];
    while buf.len() < content_length {
        let to_read = (content_length - buf.len()).min(chunk.len());
        match socket.read(&mut chunk[..to_read]).await {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => return Err("read error during body"),
        }
    }

    if buf.len() != content_length {
        log::warn!(
            "http: incomplete body ({}/{} bytes)",
            buf.len(),
            content_length
        );
        return send_response(socket, 400, "text/plain", b"incomplete body").await;
    }

    log::info!(
        "http: received {} bytes, first 8: {:02x?}",
        buf.len(),
        &buf[..buf.len().min(8)]
    );

    // Store raw bytes — main loop will decode (avoids stack pressure in HTTP task)
    METADATA_SLOT.store(buf);
    log::info!("http: metadata bytes stored, ready for main loop");
    send_response(socket, 200, "text/plain", b"ok").await
}

async fn send_response(
    socket: &mut TcpSocket<'_>,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), &'static str> {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        _ => "Error",
    };
    let mut hdr = heapless::String::<256>::new();
    write!(
        hdr,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        status, status_text, content_type, body.len()
    )
    .ok();
    write_all(socket, hdr.as_bytes()).await?;
    write_all(socket, body).await?;
    Ok(())
}

async fn send_cors_preflight(socket: &mut TcpSocket<'_>) -> Result<(), &'static str> {
    let hdr = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n";
    write_all(socket, hdr.as_bytes()).await?;
    Ok(())
}

/// Write all bytes to socket, handling partial writes.
async fn write_all(socket: &mut TcpSocket<'_>, mut buf: &[u8]) -> Result<(), &'static str> {
    while !buf.is_empty() {
        let n = socket.write(buf).await.map_err(|_| "write error")?;
        buf = &buf[n..];
    }
    Ok(())
}
