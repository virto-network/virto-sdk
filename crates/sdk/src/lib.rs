//! Virto SDK — framework-agnostic HTTP API for Substrate chains.
//!
//! Maps HTTP requests directly to sube operations:
//!
//! - `GET /query/{pallet}/{item}/{keys...}` → storage query
//! - `POST /call/{pallet}/{method}` → submit extrinsic (body = text-format args)
//! - `GET /meta` → list pallets
//! - `GET /meta/{pallet}` → pallet detail
//! - `GET /meta/{pallet}/{item}` → item type info

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

// --- Core handler ---

/// Route a request to the appropriate sube operation.
pub async fn handle<B: Backend>(chain: &mut Sube<B>, req: &Request) -> Response {
    let path = req.path.trim_start_matches('/');

    if let Some(rest) = path.strip_prefix("query/") {
        return handle_query(chain, rest).await;
    }
    if let Some(rest) = path.strip_prefix("call/") {
        return handle_call(chain, rest, &req.body).await;
    }
    if path == "meta" || path.strip_prefix("meta/").is_some() {
        let sub = path.strip_prefix("meta").unwrap_or("").trim_start_matches('/');
        return handle_meta(chain.metadata(), sub);
    }

    Response::not_found()
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

async fn handle_call<B: Backend>(_chain: &mut Sube<B>, path: &str, _body: &str) -> Response {
    if path.is_empty() {
        return Response::bad_request("usage: /call/{pallet}/{method}");
    }
    // Submitting extrinsics requires a signer — future phase.
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

// --- Formatting helpers ---

fn format_response(r: &SubeResponse) -> Response {
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
