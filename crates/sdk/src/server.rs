//! Minimal HTTP/1.1 server using smol — single-threaded, no Send required.

use smol::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use smol::net::TcpListener;
use sube::Sube;

use crate::{handle, Method, Request, Response};

pub async fn run(addr: &str, chain: &mut Sube) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    log::info!("listening on http://{addr}");

    loop {
        let (stream, peer) = listener.accept().await?;
        log::debug!("connection from {peer}");

        if let Err(e) = serve_one(chain, stream).await {
            log::warn!("request error: {e}");
        }
    }
}

async fn serve_one(chain: &mut Sube, stream: smol::net::TcpStream) -> std::io::Result<()> {
    let (reader_half, writer_half) = smol::io::split(stream);
    let mut reader = BufReader::new(reader_half);
    let mut writer = writer_half;

    // Request line: "GET /path HTTP/1.1"
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let parts: Vec<&str> = line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let method = match parts[0] {
        "GET" => Method::Get,
        "POST" => Method::Post,
        _ => {
            return send(&mut writer, &Response::text(405, "method not allowed")).await;
        }
    };

    let (path, query) = match parts[1].split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (parts[1].to_string(), String::new()),
    };

    // Read headers
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).await?;
        if header.trim().is_empty() {
            break;
        }
        let lower = header.to_ascii_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body
    let mut body_buf = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body_buf).await?;
    }

    let req = Request {
        method,
        path,
        body: String::from_utf8_lossy(&body_buf).into_owned(),
        query,
    };

    let resp = handle(chain, &req).await;
    send(&mut writer, &resp).await
}

async fn send(writer: &mut (impl AsyncWriteExt + Unpin), resp: &Response) -> std::io::Result<()> {
    let reason = match resp.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "Unknown",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        resp.status, reason, resp.content_type, resp.body.len(),
    );
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(resp.body.as_bytes()).await?;
    writer.flush().await
}
