//! Concurrent HTTP/1.1 + SSE server using smol's `LocalExecutor`.
//!
//! Single-threaded by design — `Sube` is not thread-safe and we never need
//! to be. One **core actor** task owns the `Sube` instance and serializes all
//! chain operations; per-connection tasks talk to it through a command
//! channel. The accept loop spawns a task per TCP connection so long-running
//! SSE streams don't block new clients.

use smol::channel::{self, Receiver, Sender};
use smol::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use smol::net::{TcpListener, TcpStream};
use smol::{future, LocalExecutor};
use sube::{ChainEvent, Sube};

use crate::{
    format_chain_event, format_watch_event, handle, is_sse_request, Method,
    Request, Response,
};

/// Per-subscriber channel capacity. Slow SSE clients drop events past this.
const SUB_CHANNEL_CAP: usize = 64;

enum Cmd {
    Handle {
        req: Request,
        reply: Sender<Response>,
    },
    QueryAt {
        path: String,
        hash: String,
        reply: Sender<sube::Result<sube::Response>>,
    },
    Subscribe {
        events: Sender<ChainEvent>,
    },
}

pub async fn run(addr: &str, chain: &mut Sube) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    log::info!("listening on http://{addr}");

    let (cmd_tx, cmd_rx) = channel::unbounded::<Cmd>();
    let ex = LocalExecutor::new();
    let ex = &ex;

    let core = ex.spawn(core_actor(chain, cmd_rx));

    ex.run(async move {
        let _core = core;
        accept_loop(listener, cmd_tx, ex).await
    })
    .await
}

async fn accept_loop(
    listener: TcpListener,
    cmd_tx: Sender<Cmd>,
    ex: &LocalExecutor<'_>,
) -> std::io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        log::debug!("connection from {peer}");
        let cmd_tx = cmd_tx.clone();
        ex.spawn(async move {
            if let Err(e) = serve_one(stream, cmd_tx).await {
                log::warn!("conn {peer}: {e}");
            }
        })
        .detach();
    }
}

/// Owns `Sube`. Races command receipt against `next_event`; on each event,
/// fans out to live subscribers (dropping closed senders).
async fn core_actor(chain: &mut Sube, cmd_rx: Receiver<Cmd>) {
    enum Outcome {
        Cmd(Result<Cmd, channel::RecvError>),
        Event(sube::Result<ChainEvent>),
    }

    let mut subs: Vec<Sender<ChainEvent>> = Vec::new();

    loop {
        let outcome = {
            let cmd_fut = cmd_rx.recv();
            let event_fut = chain.next_event();
            future::or(
                async { Outcome::Cmd(cmd_fut.await) },
                async { Outcome::Event(event_fut.await) },
            )
            .await
        };

        match outcome {
            Outcome::Cmd(Err(_)) => return,
            Outcome::Cmd(Ok(Cmd::Handle { req, reply })) => {
                let resp = handle(chain, &req).await;
                let _ = reply.try_send(resp);
            }
            Outcome::Cmd(Ok(Cmd::QueryAt { path, hash, reply })) => {
                let r = chain.query_at_hash(&path, &hash).await;
                let _ = reply.try_send(r);
            }
            Outcome::Cmd(Ok(Cmd::Subscribe { events })) => {
                subs.push(events);
            }
            Outcome::Event(Ok(ev)) => {
                subs.retain(|s| !s.is_closed());
                for s in &subs {
                    let _ = s.try_send(ev.clone());
                }
            }
            Outcome::Event(Err(e)) => {
                log::warn!("chain event error: {e}");
                subs.clear();
                return;
            }
        }
    }
}

async fn serve_one(stream: TcpStream, cmd_tx: Sender<Cmd>) -> std::io::Result<()> {
    let (reader_half, mut writer) = smol::io::split(stream);
    let mut reader = BufReader::new(reader_half);

    let Some(req) = parse_request(&mut reader).await? else {
        return Ok(());
    };

    if is_sse_request(&req) {
        return serve_sse(req, &mut writer, cmd_tx).await;
    }

    let (reply_tx, reply_rx) = channel::bounded(1);
    cmd_tx
        .send(Cmd::Handle { req, reply: reply_tx })
        .await
        .map_err(|e| io_err(format!("core unavailable: {e}")))?;

    let resp = reply_rx
        .recv()
        .await
        .map_err(|e| io_err(format!("core dropped reply: {e}")))?;

    send(&mut writer, &resp).await
}

async fn serve_sse(
    req: Request,
    writer: &mut (impl AsyncWriteExt + Unpin),
    cmd_tx: Sender<Cmd>,
) -> std::io::Result<()> {
    let watch_path = req.query_param("watch").map(String::from);

    let (ev_tx, ev_rx) = channel::bounded(SUB_CHANNEL_CAP);
    cmd_tx
        .send(Cmd::Subscribe { events: ev_tx })
        .await
        .map_err(|e| io_err(format!("core unavailable: {e}")))?;

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

    while let Ok(event) = ev_rx.recv().await {
        let frame = format_chain_event(&event);
        if writer.write_all(frame.as_bytes()).await.is_err() {
            break;
        }

        if let (Some(p), ChainEvent::NewBlock { hash, .. }) = (&watch_path, &event) {
            let (tx, rx) = channel::bounded(1);
            if cmd_tx
                .send(Cmd::QueryAt {
                    path: p.clone(),
                    hash: hash.clone(),
                    reply: tx,
                })
                .await
                .is_err()
            {
                break;
            }
            if let Ok(Ok(resp)) = rx.recv().await {
                let current_raw: Vec<u8> = match &resp {
                    sube::Response::Value(e, _) => e.data.clone(),
                    _ => Vec::new(),
                };
                if current_raw != prev_raw {
                    let frame = format_watch_event(p, &resp);
                    if writer.write_all(frame.as_bytes()).await.is_err() {
                        break;
                    }
                    prev_raw = current_raw;
                }
            }
        }

        if writer.flush().await.is_err() {
            break;
        }
    }
    Ok(())
}

fn io_err(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::other(msg.into())
}

// --- HTTP parsing ---

async fn parse_request(
    reader: &mut (impl AsyncBufReadExt + Unpin),
) -> std::io::Result<Option<Request>> {
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
        resp.status,
        reason,
        resp.content_type,
        resp.body.len(),
    );
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(resp.body.as_bytes()).await?;
    writer.flush().await
}
