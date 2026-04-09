//! Minimal HTTP/1.1 server using smol — single-threaded, no Send required.

use smol::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use smol::net::TcpListener;
use sube::Sube;

use crate::{format_chain_event, format_watch_event, handle, is_sse_request, Method, Request, Response};

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

    let req = parse_request(&mut reader).await?;
    let Some(req) = req else { return Ok(()) };

    if is_sse_request(&req) {
        return serve_sse(chain, &req, &mut writer).await;
    }

    let resp = handle(chain, &req).await;
    send(&mut writer, &resp).await
}

/// SSE: stream chain events (and optionally re-query on each block).
async fn serve_sse(
    chain: &mut Sube,
    req: &Request,
    writer: &mut (impl AsyncWriteExt + Unpin),
) -> std::io::Result<()> {
    let watch_path = req.query_param("watch");

    // SSE preamble
    writer
        .write_all(
            b"HTTP/1.1 200 OK\r\n\
              Content-Type: text/event-stream\r\n\
              Cache-Control: no-cache\r\n\
              Connection: keep-alive\r\n\r\n",
        )
        .await?;
    writer.flush().await?;

    let mut prev_raw: Vec<u8> = Vec::new();

    loop {
        let event = match chain.next_event().await {
            Ok(e) => e,
            Err(e) => {
                let frame = format!("event: error\ndata: {e}\n\n");
                writer.write_all(frame.as_bytes()).await?;
                writer.flush().await?;
                break;
            }
        };

        // Always emit the chain event
        let frame = format_chain_event(&event);
        if writer.write_all(frame.as_bytes()).await.is_err() {
            break; // client disconnected
        }

        // If watching a query, re-run it on new blocks
        if let (Some(path), sube::ChainEvent::NewBlock { ref hash, .. }) = (watch_path, &event) {
            if let Ok(resp) = chain.query_at_hash(path, hash).await {
                // Only emit if the value changed
                let current_raw: Vec<u8> = match &resp {
                    sube::Response::Value(e, _) => e.data.clone(),
                    _ => Vec::new(),
                };
                if current_raw != prev_raw {
                    let frame = format_watch_event(path, &resp);
                    if writer.write_all(frame.as_bytes()).await.is_err() {
                        break;
                    }
                    prev_raw = current_raw;
                }
            }
        }

        if writer.flush().await.is_err() {
            break; // client disconnected
        }
    }
    Ok(())
}

// --- HTTP parsing ---

async fn parse_request(
    reader: &mut (impl AsyncBufReadExt + Unpin),
) -> std::io::Result<Option<Request>> {
    // Request line: "GET /path HTTP/1.1"
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let parts: Vec<&str> = line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Ok(None);
    }

    let method = match parts[0] {
        "GET" => Method::Get,
        "POST" => Method::Post,
        _ => return Ok(None),
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

    Ok(Some(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body_buf).into_owned(),
        query,
    }))
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
