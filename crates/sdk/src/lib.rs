//! Virto SDK — framework-agnostic HTTP API for Substrate chains.
//!
//! Maps HTTP requests directly to sube operations:
//!
//! - `GET  /query/{pallet}/{item}/{keys...}` → storage query
//! - `POST /call/{pallet}/{method}` → submit extrinsic (body = text-format args)
//! - `GET  /meta[/{pallet}[/{item}]]` → metadata introspection
//! - `GET  /events[?watch={path}]` → SSE stream (chain events + optional watched query)

use sube::{Backend, Metadata, Response as SubeResponse, Sube};

#[cfg(feature = "server")]
pub mod server;

// --- Request / Response types (no framework dependency) ---

pub enum Method {
    Get,
    Post,
}

pub struct Request {
    pub method: Method,
    pub path: String,
    pub body: String,
    pub query: String,
}

impl Request {
    pub fn query_param(&self, key: &str) -> Option<&str> {
        self.query
            .split('&')
            .find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                (k == key).then_some(v)
            })
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Response {
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self { status, content_type: "text/plain; charset=utf-8", body: body.into() }
    }
    pub fn not_found() -> Self { Self::text(404, "not found") }
    pub fn bad_request(msg: impl Into<String>) -> Self { Self::text(400, msg) }
    pub fn error(msg: impl Into<String>) -> Self { Self::text(500, msg) }
}

// --- Core handler (non-streaming) ---

/// Route a request to the appropriate sube operation.
/// Streaming endpoints (/events) are handled at the server layer.
pub async fn handle<B: Backend>(chain: &mut Sube<B>, req: &Request) -> Response {
    let path = req.path.trim_start_matches('/');

    if let Some(rest) = path.strip_prefix("query/") {
        return handle_query(chain, rest).await;
    }
    if let Some(rest) = path.strip_prefix("call/") {
        return handle_call(rest, &req.body);
    }
    if path == "meta" || path.starts_with("meta/") {
        let sub = path.strip_prefix("meta").unwrap_or("").trim_start_matches('/');
        return handle_meta(chain.metadata(), sub);
    }
    if path == "events" || path.starts_with("events?") {
        return Response::bad_request("use an SSE client (EventSource) for /events");
    }

    Response::not_found()
}

pub fn is_sse_request(req: &Request) -> bool {
    req.path.trim_start_matches('/').starts_with("events")
}

async fn handle_query<B: Backend>(chain: &mut Sube<B>, path: &str) -> Response {
    if path.is_empty() {
        return Response::bad_request("usage: /query/{pallet}/{item}[/{keys}]");
    }
    match chain.query(path).await {
        Ok(ref r) => format_response(r),
        Err(e) => Response::error(e.to_string()),
    }
}

fn handle_call(path: &str, _body: &str) -> Response {
    if path.is_empty() {
        return Response::bad_request("usage: /call/{pallet}/{method}");
    }
    // Signing is wired at the server layer where the concrete assembler type
    // is known (SignerFn, PassAuthenticator, etc.). The framework-agnostic
    // core doesn't handle /call yet.
    Response::text(501, "signing not yet configured")
}

fn handle_meta(meta: &Metadata, sub_path: &str) -> Response {
    if sub_path.is_empty() {
        let names: Vec<&str> = meta.pallets.iter().map(|p| p.name.as_str()).collect();
        return Response::text(200, names.join("\n"));
    }

    let (pallet_name, item_name) = match sub_path.split_once('/') {
        Some((p, i)) => (p, Some(i)),
        None => (sub_path, None),
    };

    let pallet_name = sube::util::to_camel(pallet_name);
    let pallet = match meta.pallet_by_name(&pallet_name) {
        Some(p) => p,
        None => return Response::not_found(),
    };

    match item_name {
        None => format_pallet(pallet),
        Some(item) => format_item(pallet, item),
    }
}

// --- SSE event formatting ---

/// Format a chain event as an SSE frame (returns `event: ...\ndata: ...\n\n`).
pub fn format_chain_event(event: &sube::ChainEvent) -> String {
    match event {
        sube::ChainEvent::NewBlock { hash, number, .. } => {
            format!("event: new_block\ndata: #{number} {hash}\n\n")
        }
        sube::ChainEvent::Finalized { hashes, .. } => {
            let data = hashes.join(",");
            format!("event: finalized\ndata: {data}\n\n")
        }
        sube::ChainEvent::BestBlock { hash } => {
            format!("event: best_block\ndata: {hash}\n\n")
        }
    }
}

/// Format a watched-query result as an SSE frame.
pub fn format_watch_event(path: &str, result: &SubeResponse) -> String {
    let text = match result {
        SubeResponse::Value(entry, meta) => {
            entry.to_text(&meta.registry).unwrap_or_else(|e| e.to_string())
        }
        SubeResponse::None => "(none)".into(),
        _ => "(complex)".into(),
    };
    format!("event: query\ndata: {path} {text}\n\n")
}

// --- Formatting helpers ---

pub fn format_response(r: &SubeResponse) -> Response {
    match r {
        SubeResponse::None => Response::text(200, "(none)"),
        SubeResponse::Void => Response::text(200, ""),
        SubeResponse::Value(entry, meta) => match entry.to_text(&meta.registry) {
            Ok(text) => Response::text(200, text),
            Err(e) => Response::error(e.to_string()),
        },
        SubeResponse::ValueSet(items, meta) => {
            let mut out = String::new();
            for (keys, value) in items {
                let ks: Vec<String> =
                    keys.iter().filter_map(|k| k.to_text(&meta.registry).ok()).collect();
                let kd = ks.join(", ");
                match value {
                    Some(v) => {
                        let val = v.to_text(&meta.registry).unwrap_or_else(|_| "(error)".into());
                        out.push_str(&format!("[{kd}] {val}\n"));
                    }
                    None => out.push_str(&format!("[{kd}] (none)\n")),
                }
            }
            Response::text(200, out)
        }
        SubeResponse::Meta(meta) => {
            let names: Vec<&str> = meta.pallets.iter().map(|p| p.name.as_str()).collect();
            Response::text(200, names.join("\n"))
        }
    }
}

fn format_pallet(pallet: &sube::metadata::PalletMeta) -> Response {
    let mut out = String::new();
    if let Some(storage) = &pallet.storage {
        out.push_str("storage:\n");
        for e in &storage.entries {
            out.push_str(&format!("  {}\n", e.name));
        }
    }
    if !pallet.constants.is_empty() {
        out.push_str("constants:\n");
        for c in &pallet.constants {
            out.push_str(&format!("  {}\n", c.name));
        }
    }
    if pallet.calls_ty.is_some() {
        out.push_str("calls: yes\n");
    }
    Response::text(200, out)
}

fn format_item(pallet: &sube::metadata::PalletMeta, item: &str) -> Response {
    if let Some(storage) = &pallet.storage {
        if let Some(e) = storage.entries.iter().find(|e| e.name.eq_ignore_ascii_case(item)) {
            return Response::text(200, format!("{:?}", e.ty));
        }
    }
    if let Some(c) = pallet.constants.iter().find(|c| c.name.eq_ignore_ascii_case(item)) {
        return Response::text(200, format!("constant type_id={}", c.ty));
    }
    Response::not_found()
}
