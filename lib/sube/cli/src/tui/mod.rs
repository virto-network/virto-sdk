use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use ratatui::widgets::*;

use sube::metadata::{PalletMeta, StorageEntryType};
use sube::Metadata;

mod form;
mod types;

use form::{field_from_type, FormState};

// --- Messages between UI and chain task ---

enum ToChain {
    Query(String),
    #[allow(dead_code)]
    QueryAtHash(String, String),
    FetchBlockDetail(String), // block hash
}

enum FromChain {
    Block(BlockInfo),
    Finalized(Vec<String>),
    StorageResult(String),
    StorageError(String),
    BlockDetail(String, String), // hash, events text
}

// --- Block info ---

struct BlockInfo {
    number: u64,
    hash: String,
    finalized: bool,
    has_extrinsics: bool,
    event_count: usize,
}

// --- Panels ---

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Pallets,
    Storage,
    Calls,
    Blocks,
}

impl Panel {
    fn next(self) -> Self {
        match self {
            Panel::Pallets => Panel::Storage,
            Panel::Storage => Panel::Calls,
            Panel::Calls => Panel::Blocks,
            Panel::Blocks => Panel::Pallets,
        }
    }
    fn prev(self) -> Self {
        match self {
            Panel::Pallets => Panel::Blocks,
            Panel::Storage => Panel::Pallets,
            Panel::Calls => Panel::Storage,
            Panel::Blocks => Panel::Calls,
        }
    }
}

// --- Block detail ---

struct BlockDetail {
    block_idx: usize,
    events: Option<String>,
    /// Hash of block whose detail we last requested (for debounce)
    pending_hash: Option<(String, std::time::Instant)>,
}

// --- Focus ---

enum Focus {
    Navigate,
    Form,
    Search,
    BlockDetail,
}

// --- App (UI-only state) ---

struct App {
    chain_url: String,
    meta: sube::Arc<Metadata>,

    all_pallets: Vec<String>,
    pallets: Vec<String>,
    pallet_idx: usize,

    storage_items: Vec<String>,
    storage_idx: usize,
    storage_form: Option<FormState>,
    storage_result: Option<String>,

    call_items: Vec<String>,
    call_idx: usize,
    call_form: Option<FormState>,
    call_result: Option<String>,

    panel: Panel,
    focus: Focus,
    search_query: String,

    recent_blocks: Vec<BlockInfo>,
    block_detail: Option<BlockDetail>,

    error: Option<String>,
    should_quit: bool,

    to_chain: mpsc::Sender<ToChain>,
}

impl App {
    fn current_pallet(&self) -> Option<&PalletMeta> {
        let name = self.pallets.get(self.pallet_idx)?;
        self.meta.pallet_by_name(name)
    }

    fn refresh_pallet(&mut self) {
        self.storage_items.clear();
        self.storage_idx = 0;
        self.storage_form = None;
        self.storage_result = None;
        self.call_items.clear();
        self.call_idx = 0;
        self.call_form = None;
        self.call_result = None;
        self.error = None;

        let name = match self.pallets.get(self.pallet_idx) {
            Some(n) => n.clone(),
            None => return,
        };
        let Some(pallet) = self.meta.pallet_by_name(&name) else {
            return;
        };

        if let Some(storage) = &pallet.storage {
            self.storage_items = storage.entries.iter().map(|e| e.name.clone()).collect();
        }

        let calls_ty = pallet.calls_ty;
        if let Some(calls_ty) = calls_ty {
            if let Some(sube::scales::TypeDef::Variant(vdef)) =
                self.meta.registry.resolve(calls_ty)
            {
                self.call_items = vdef.variants.iter().map(|v| v.name.clone()).collect();
            }
        }
    }

    fn build_storage_form(&mut self) {
        self.storage_form = None;
        self.storage_result = None;
        self.error = None;

        let pallet = match self.current_pallet() {
            Some(p) => p,
            None => return,
        };
        let item_name = match self.storage_items.get(self.storage_idx) {
            Some(n) => n.clone(),
            None => return,
        };

        let entry = pallet
            .storage
            .as_ref()
            .and_then(|s| s.entries.iter().find(|e| e.name == item_name));
        if let Some(entry) = entry {
            let reg = sube::Arc::clone(&self.meta);
            match &entry.ty {
                StorageEntryType::Plain(_) => {
                    self.storage_form =
                        Some(FormState::new(vec![], sube::Arc::new(reg.registry.clone())));
                }
                StorageEntryType::Map { hashers, key, .. } => {
                    let key_types =
                        types::extract_key_types(*key, hashers.len(), &self.meta.registry);
                    let fields: Vec<form::Field> = key_types
                        .iter()
                        .enumerate()
                        .map(|(i, ty_id)| {
                            field_from_type(&format!("key{i}"), *ty_id, &self.meta.registry)
                        })
                        .collect();
                    self.storage_form =
                        Some(FormState::new(fields, sube::Arc::new(reg.registry.clone())));
                }
            }
        }
    }

    fn build_call_form(&mut self) {
        self.call_form = None;
        self.call_result = None;
        self.error = None;

        let pallet = match self.current_pallet() {
            Some(p) => p,
            None => return,
        };
        let item_name = match self.call_items.get(self.call_idx) {
            Some(n) => n.clone(),
            None => return,
        };

        if let Some(calls_ty) = pallet.calls_ty {
            if let Some(sube::scales::TypeDef::Variant(vdef)) =
                self.meta.registry.resolve(calls_ty)
            {
                if let Some(variant) = vdef.variants.iter().find(|v| v.name == item_name) {
                    let fields = match &variant.fields {
                        sube::scales::registry::Fields::Unit => vec![],
                        sube::scales::registry::Fields::NewType(ty_id) => {
                            vec![field_from_type("value", *ty_id, &self.meta.registry)]
                        }
                        sube::scales::registry::Fields::Tuple(ids) => ids
                            .iter()
                            .enumerate()
                            .map(|(i, id)| field_from_type(&format!("field{i}"), *id, &self.meta.registry))
                            .collect(),
                        sube::scales::registry::Fields::Struct(fields) => fields
                            .iter()
                            .map(|f| field_from_type(&f.name, f.ty, &self.meta.registry))
                            .collect(),
                    };
                    self.call_form = Some(FormState::new(
                        fields,
                        sube::Arc::new(self.meta.registry.clone()),
                    ));
                }
            }
        }
    }

    fn execute_storage_query(&mut self) {
        self.error = None;
        self.storage_result = Some("querying...".into());

        let pallet = match self.current_pallet() {
            Some(p) => p,
            None => return,
        };
        let item_name = match self.storage_items.get(self.storage_idx) {
            Some(n) => n.clone(),
            None => return,
        };

        let values: Vec<String> = self
            .storage_form
            .as_ref()
            .map(|f| f.values())
            .unwrap_or_default();
        let keys_part = if values.is_empty() || values.iter().all(|v| v.is_empty()) {
            String::new()
        } else {
            format!("/{}", values.join("/"))
        };
        let path = format!("{}/{}{keys_part}", pallet.name, item_name);
        let _ = self.to_chain.send(ToChain::Query(path));
    }

    fn execute_call(&mut self) {
        self.error = None;
        self.call_result = None;

        let pallet = match self.current_pallet() {
            Some(p) => p,
            None => return,
        };
        let item_name = match self.call_items.get(self.call_idx) {
            Some(n) => n.clone(),
            None => return,
        };

        let values: Vec<String> = self
            .call_form
            .as_ref()
            .map(|f| f.values())
            .unwrap_or_default();
        let field_names: Vec<String> = self
            .call_form
            .as_ref()
            .map(|f| f.field_names())
            .unwrap_or_default();
        let parts: Vec<String> = field_names
            .iter()
            .zip(values.iter())
            .map(|(name, val)| format!("{name}:{val}"))
            .collect();
        let body = if parts.is_empty() {
            String::new()
        } else {
            format!("({})", parts.join(";"))
        };
        self.call_result = Some(format!(
            "sube -c {} {}/{} --body '{body}'",
            self.chain_url, pallet.name, item_name,
        ));
    }

    fn process_chain_message(&mut self, msg: FromChain) {
        match msg {
            FromChain::Block(block) => {
                self.recent_blocks.push(block);
                if self.recent_blocks.len() > 200 {
                    self.recent_blocks.remove(0);
                    if let Some(ref mut d) = self.block_detail {
                        d.block_idx = d.block_idx.saturating_sub(1);
                    }
                }
            }
            FromChain::Finalized(hashes) => {
                for hash in &hashes {
                    if let Some(b) = self.recent_blocks.iter_mut().find(|b| b.hash == *hash) {
                        b.finalized = true;
                    }
                }
            }
            FromChain::StorageResult(text) => {
                self.storage_result = Some(text);
            }
            FromChain::StorageError(e) => {
                self.error = Some(e);
                self.storage_result = None;
            }
            FromChain::BlockDetail(hash, events) => {
                if let Some(ref mut d) = self.block_detail {
                    if let Some(idx) = self.recent_blocks.iter().position(|b| b.hash == hash) {
                        if d.block_idx == idx {
                            d.events = Some(events);
                        }
                    }
                }
            }
        }
    }

    fn open_block_detail(&mut self) {
        if self.recent_blocks.is_empty() {
            return;
        }
        let idx = self.recent_blocks.len() - 1;
        let hash = self.recent_blocks[idx].hash.clone();
        self.block_detail = Some(BlockDetail {
            block_idx: idx,
            events: Some("loading...".into()),
            pending_hash: None,
        });
        self.focus = Focus::BlockDetail;
        let _ = self.to_chain.send(ToChain::FetchBlockDetail(hash));
    }

    fn navigate_block_detail(&mut self, delta: isize) {
        let Some(ref mut detail) = self.block_detail else { return };
        let new_idx = (detail.block_idx as isize + delta)
            .clamp(0, self.recent_blocks.len() as isize - 1) as usize;
        if new_idx != detail.block_idx {
            detail.block_idx = new_idx;
            detail.events = None;
            // Debounce: record the hash and time, fetch after 200ms of no navigation
            let hash = self.recent_blocks[new_idx].hash.clone();
            detail.pending_hash = Some((hash, std::time::Instant::now()));
        }
    }

    /// Check if a debounced block detail fetch is ready to fire.
    fn flush_pending_detail(&mut self) {
        let should_fetch = self.block_detail.as_ref().and_then(|d| {
            d.pending_hash.as_ref().and_then(|(hash, when)| {
                if when.elapsed() >= std::time::Duration::from_millis(200) {
                    Some(hash.clone())
                } else {
                    None
                }
            })
        });
        if let Some(hash) = should_fetch {
            if let Some(ref mut detail) = self.block_detail {
                detail.events = Some("loading...".into());
                detail.pending_hash = None;
            }
            let _ = self.to_chain.send(ToChain::FetchBlockDetail(hash));
        }
    }

    fn apply_filter(&mut self) {
        let q = self.search_query.to_lowercase();
        if q.is_empty() {
            self.pallets = self.all_pallets.clone();
        } else {
            self.pallets = self
                .all_pallets
                .iter()
                .filter(|name| fuzzy_match(&q, name))
                .cloned()
                .collect();
        }
        if self.pallet_idx >= self.pallets.len() {
            self.pallet_idx = self.pallets.len().saturating_sub(1);
        }
    }
}

// --- Chain task ---

fn spawn_chain_task(
    mut chain: sube::Sube,
    from_ui: mpsc::Receiver<ToChain>,
    to_ui: smol::channel::Sender<FromChain>,
) {
    thread::spawn(move || {
        smol::block_on(async {
            loop {
                // Check for UI commands (non-blocking)
                while let Ok(cmd) = from_ui.try_recv() {
                    match cmd {
                        ToChain::Query(path) => {
                            match chain.query(&path).await {
                                Ok(resp) => {
                                    let _ = to_ui.send(FromChain::StorageResult(
                                        format_response(resp),
                                    )).await;
                                }
                                Err(e) => {
                                    let _ = to_ui.send(FromChain::StorageError(
                                        format!("{e}"),
                                    )).await;
                                }
                            }
                        }
                        ToChain::QueryAtHash(path, hash) => {
                            match chain.query_at_hash(&path, &hash).await {
                                Ok(resp) => {
                                    let _ = to_ui.send(FromChain::StorageResult(
                                        format_response(resp),
                                    )).await;
                                }
                                Err(e) => {
                                    let _ = to_ui.send(FromChain::StorageError(
                                        format!("{e}"),
                                    )).await;
                                }
                            }
                        }
                        ToChain::FetchBlockDetail(hash) => {
                            let events = match chain.query_at_hash("system/events", &hash).await {
                                Ok(resp) => {
                                    match resp.to_json() {
                                        Ok(Some(json)) => format_events_detail(&json),
                                        Ok(None) => "(no events)".into(),
                                        Err(e) => format!("decode error: {e}"),
                                    }
                                }
                                Err(e) => format!("error: {e}"),
                            };
                            let _ = to_ui.send(FromChain::BlockDetail(hash, events)).await;
                        }
                    }
                }

                // Wait for next chain event
                match chain.next_event().await {
                    Ok(sube::ChainEvent::NewBlock { hash, .. }) => {
                        let number = chain.header(&hash).await.map(|h| h.number).unwrap_or(0);
                        let (event_count, interesting) = match chain.query_at_hash("system/events", &hash).await {
                            Ok(sube::Response::Value(entry, meta)) => {
                                match entry.to_json(&meta.registry) {
                                    Ok(json) => {
                                        let events = json.as_array();
                                        let total = events.map(|a| a.len()).unwrap_or(0);
                                        let interesting = events
                                            .map(|arr| arr.iter().filter(|e| is_interesting_event(e)).count())
                                            .unwrap_or(0);
                                        (total, interesting)
                                    }
                                    Err(_) => (0, 0),
                                }
                            }
                            _ => (0, 0),
                        };
                        let _ = to_ui.send(FromChain::Block(BlockInfo {
                            number,
                            hash,
                            finalized: false,
                            has_extrinsics: interesting > 0,
                            event_count,
                        })).await;
                    }
                    Ok(sube::ChainEvent::Finalized { hashes, .. }) => {
                        let _ = to_ui.send(FromChain::Finalized(hashes)).await;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        // Connection error — wait a bit before retrying
                        smol::Timer::after(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
        })
    });
}

// --- Helpers ---

fn fuzzy_match(query: &str, target: &str) -> bool {
    let target_lower = target.to_lowercase();
    let mut chars = target_lower.chars();
    for qc in query.chars() {
        if chars.by_ref().find(|&c| c == qc).is_none() {
            return false;
        }
    }
    true
}

/// Filter out routine system events that appear in every block.
fn is_interesting_event(event: &serde_json::Value) -> bool {
    let event_body = match event.get("event") {
        Some(e) => e,
        None => return false,
    };
    // The event is an enum variant like {"Balances": {"Transfer": {...}}}
    let pallet = match event_body.as_object().and_then(|o| o.keys().next()) {
        Some(p) => p,
        None => return false,
    };
    // Skip routine pallets that fire in every block
    !matches!(
        pallet.as_str(),
        "System" | "ParachainSystem" | "TransactionPayment"
            | "MessageQueue" | "CumulusXcm"
    )
}

/// Format block events JSON into readable detail text.
fn format_events_detail(json: &serde_json::Value) -> String {
    let events = match json.as_array() {
        Some(arr) => arr,
        None => return "(invalid events)".into(),
    };

    let mut out = String::new();
    for (i, event) in events.iter().enumerate() {
        let phase = match event.get("phase") {
            Some(p) => {
                if let Some(n) = p.get("ApplyExtrinsic") {
                    format!("extrinsic #{n}")
                } else if p.get("Initialization").is_some() {
                    "initialization".into()
                } else if p.get("Finalization").is_some() {
                    "finalization".into()
                } else {
                    format!("{p}")
                }
            }
            None => "?".into(),
        };

        let (pallet, event_name, fields) = match event.get("event").and_then(|e| e.as_object()) {
            Some(obj) => {
                if let Some((pallet, inner)) = obj.iter().next() {
                    match inner.as_object().and_then(|o| o.iter().next()) {
                        Some((name, data)) => (pallet.as_str(), name.as_str(), Some(data)),
                        None => (pallet.as_str(), "?", None),
                    }
                } else {
                    ("?", "?", None)
                }
            }
            None => ("?", "?", None),
        };

        out.push_str(&format!("{}. {} » {pallet}::{event_name}\n", i + 1, phase));

        if let Some(data) = fields {
            if let Some(obj) = data.as_object() {
                for (key, val) in obj {
                    let val_str = format_value(val);
                    out.push_str(&format!("   {key}: {val_str}\n"));
                }
            } else {
                let val_str = format_value(data);
                if val_str != "null" {
                    out.push_str(&format!("   {val_str}\n"));
                }
            }
        }
        out.push('\n');
    }
    out
}

/// Format a JSON value concisely for display.
fn format_value(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => {
            if s.starts_with("0x") && s.len() > 20 {
                // Truncate long hex values
                format!("{}…{}", &s[..10], &s[s.len() - 8..])
            } else {
                s.clone()
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Object(obj) => {
            if obj.len() == 1 {
                // Enum variant like {"Id": "0x1234..."}
                let (k, v) = obj.iter().next().unwrap();
                let v_str = format_value(v);
                if v_str == "null" {
                    k.clone()
                } else {
                    format!("{k}({v_str})")
                }
            } else if obj.len() <= 3 {
                let parts: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", format_value(v)))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            } else {
                format!("{{ {} fields }}", obj.len())
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.len() <= 3 {
                let parts: Vec<String> = arr.iter().map(format_value).collect();
                format!("[{}]", parts.join(", "))
            } else {
                format!("[{} items]", arr.len())
            }
        }
    }
}

fn format_response(resp: sube::Response) -> String {
    match resp {
        sube::Response::None => "(none)".into(),
        sube::Response::Value(entry, meta) => entry
            .to_text(&meta.registry)
            .unwrap_or_else(|e| format!("error: {e}")),
        sube::Response::ValueSet(items, meta) => {
            let mut out = String::new();
            for (keys, value) in items {
                let key_strs: Vec<String> = keys
                    .iter()
                    .filter_map(|k| k.to_text(&meta.registry).ok())
                    .collect();
                let key_display = key_strs.join(", ");
                match value {
                    Some(v) => {
                        let text = v
                            .to_text(&meta.registry)
                            .unwrap_or_else(|e| format!("error: {e}"));
                        out.push_str(&format!("[{key_display}] {text}\n"));
                    }
                    None => out.push_str(&format!("[{key_display}] (none)\n")),
                }
            }
            out
        }
        sube::Response::Void => "(submitted)".into(),
        sube::Response::Meta(_) => "(metadata)".into(),
    }
}

// --- Entry point ---

pub async fn run(chain_url: &str) -> Result<()> {
    eprintln!("Connecting to {chain_url}...");
    let chain = sube::Sube::connect(chain_url).await?;
    eprintln!("Connected, loading metadata...");
    let meta = chain.metadata_arc();

    // Channels: UI → chain (sync mpsc), chain → UI (async smol channel)
    let (ui_tx, chain_rx) = mpsc::channel::<ToChain>();
    let (chain_tx, ui_rx) = smol::channel::unbounded::<FromChain>();

    spawn_chain_task(chain, chain_rx, chain_tx);

    enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let mut all_pallets: Vec<String> = meta.pallets.iter().map(|p| p.name.clone()).collect();
    all_pallets.sort();
    let pallets = all_pallets.clone();

    let mut app = App {
        chain_url: chain_url.into(),
        meta,
        all_pallets,
        pallets,
        pallet_idx: 0,
        storage_items: vec![],
        storage_idx: 0,
        storage_form: None,
        storage_result: None,
        call_items: vec![],
        call_idx: 0,
        call_form: None,
        call_result: None,
        panel: Panel::Pallets,
        focus: Focus::Navigate,
        search_query: String::new(),
        recent_blocks: vec![],
        block_detail: None,
        error: None,
        should_quit: false,
        to_chain: ui_tx,
    };
    app.refresh_pallet();

    loop {
        terminal.draw(|f| draw(f, &app))?;

        // Drain chain messages (non-blocking)
        while let Ok(msg) = ui_rx.try_recv() {
            app.process_chain_message(msg);
        }
        // Fire debounced block detail fetch if ready
        app.flush_pending_detail();

        // Poll terminal input (short timeout to stay responsive)
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    app.should_quit = true;
                }
                if key.code == KeyCode::Char('q')
                    && matches!(app.focus, Focus::Navigate)
                {
                    app.should_quit = true;
                }
                handle_key(&mut app, key.code);
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn handle_key(app: &mut App, key: KeyCode) {
    match app.focus {
        Focus::Navigate => handle_navigate(app, key),
        Focus::Form => handle_form(app, key),
        Focus::Search => handle_search(app, key),
        Focus::BlockDetail => handle_block_detail(app, key),
    }
}

fn handle_navigate(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Tab => app.panel = app.panel.next(),
        KeyCode::BackTab => app.panel = app.panel.prev(),
        KeyCode::Up | KeyCode::Char('k') => match app.panel {
            Panel::Pallets => {
                if app.pallet_idx > 0 {
                    app.pallet_idx -= 1;
                    app.refresh_pallet();
                }
            }
            Panel::Storage => {
                if app.storage_idx > 0 { app.storage_idx -= 1; }
            }
            Panel::Calls => {
                if app.call_idx > 0 { app.call_idx -= 1; }
            }
            Panel::Blocks => {}
        },
        KeyCode::Down | KeyCode::Char('j') => match app.panel {
            Panel::Pallets => {
                if app.pallet_idx + 1 < app.pallets.len() {
                    app.pallet_idx += 1;
                    app.refresh_pallet();
                }
            }
            Panel::Storage => {
                if app.storage_idx + 1 < app.storage_items.len() { app.storage_idx += 1; }
            }
            Panel::Calls => {
                if app.call_idx + 1 < app.call_items.len() { app.call_idx += 1; }
            }
            Panel::Blocks => {}
        },
        KeyCode::Right | KeyCode::Char('l') => app.panel = app.panel.next(),
        KeyCode::Left | KeyCode::Char('h') => app.panel = app.panel.prev(),
        KeyCode::Enter => match app.panel {
            Panel::Storage => {
                app.build_storage_form();
                if let Some(ref f) = app.storage_form {
                    if f.is_empty() {
                        app.execute_storage_query();
                    } else {
                        app.focus = Focus::Form;
                    }
                }
            }
            Panel::Calls => {
                app.build_call_form();
                if let Some(ref f) = app.call_form {
                    if f.is_empty() {
                        app.execute_call();
                    } else {
                        app.focus = Focus::Form;
                    }
                }
            }
            Panel::Blocks => app.open_block_detail(),
            _ => {}
        },
        KeyCode::Char('/') => {
            app.search_query.clear();
            app.focus = Focus::Search;
        }
        _ => {}
    }
}

fn handle_form(app: &mut App, key: KeyCode) {
    let form = match app.panel {
        Panel::Storage => &mut app.storage_form,
        Panel::Calls => &mut app.call_form,
        _ => { app.focus = Focus::Navigate; return; }
    };
    let Some(form) = form.as_mut() else {
        app.focus = Focus::Navigate;
        return;
    };
    match key {
        KeyCode::Esc => {
            app.focus = Focus::Navigate;
            match app.panel {
                Panel::Storage => app.storage_form = None,
                Panel::Calls => app.call_form = None,
                _ => {}
            }
        }
        KeyCode::Enter => {
            if form.is_on_last() {
                match app.panel {
                    Panel::Storage => app.execute_storage_query(),
                    Panel::Calls => app.execute_call(),
                    _ => {}
                }
                app.focus = Focus::Navigate;
            } else {
                form.next_field();
            }
        }
        KeyCode::Up | KeyCode::BackTab => form.prev_field(),
        KeyCode::Down | KeyCode::Tab => form.next_field(),
        KeyCode::Right => form.toggle(),
        KeyCode::Left => form.toggle_back(),
        KeyCode::Char(' ') => form.toggle(),
        KeyCode::Char('+') => form.add_list_item(),
        KeyCode::Char('-') => form.remove_list_item(),
        KeyCode::Backspace => form.backspace(),
        KeyCode::Char(c) => form.push_char(c),
        _ => {}
    }
}

fn handle_search(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => {
            app.search_query.clear();
            app.pallets = app.all_pallets.clone();
            app.focus = Focus::Navigate;
        }
        KeyCode::Enter => app.focus = Focus::Navigate,
        KeyCode::Backspace => { app.search_query.pop(); app.apply_filter(); }
        KeyCode::Char(c) => { app.search_query.push(c); app.apply_filter(); }
        _ => {}
    }
}

fn handle_block_detail(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.block_detail = None;
            app.focus = Focus::Navigate;
        }
        // List is displayed newest-first (reversed), so Up = newer = higher index
        KeyCode::Up | KeyCode::Char('k') => app.navigate_block_detail(1),
        KeyCode::Down | KeyCode::Char('j') => app.navigate_block_detail(-1),
        _ => {}
    }
}

// --- Drawing ---

fn draw(f: &mut Frame, app: &App) {
    if let Some(ref detail) = app.block_detail {
        draw_block_detail_dialog(f, app, detail);
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    let latest = app.recent_blocks.last()
        .map(|b| format!(" · block #{}", b.number))
        .unwrap_or_default();
    f.render_widget(
        Paragraph::new(format!(" sube · {}{latest}", app.chain_url)).bold().fg(Color::Cyan),
        outer[0],
    );

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(40),
            Constraint::Percentage(40),
        ])
        .split(outer[1]);

    draw_pallets(f, app, body[0]);
    draw_storage(f, app, body[1]);
    draw_calls(f, app, body[2]);
    draw_blocks(f, app, outer[2]);

    let help = match (&app.focus, &app.panel) {
        (Focus::Search, _) => " / filter  Enter confirm  Esc cancel",
        (Focus::Form, _) => " ↑↓ fields  ←→ enum  Space toggle  +/- list  Enter next/submit  Esc cancel",
        (Focus::BlockDetail, _) => " ↑↓ navigate  Esc back",
        (_, Panel::Pallets) => " ↑↓ navigate  Tab panel  / filter  q quit",
        (_, Panel::Storage) => " ↑↓ navigate  Tab panel  Enter query  q quit",
        (_, Panel::Calls) => " ↑↓ navigate  Tab panel  Enter select  q quit",
        (_, Panel::Blocks) => " Tab panel  Enter details  q quit",
    };
    if let Some(ref err) = app.error {
        f.render_widget(Paragraph::new(format!(" error: {err}")).fg(Color::Red), outer[3]);
    } else {
        f.render_widget(Paragraph::new(help).fg(Color::DarkGray), outer[3]);
    }
}

fn panel_border(app: &App, panel: Panel) -> Style {
    if app.panel == panel { Style::default().fg(Color::Cyan) }
    else { Style::default().fg(Color::DarkGray) }
}

fn draw_pallets(f: &mut Frame, app: &App, area: Rect) {
    let title = if matches!(app.focus, Focus::Search) {
        format!("Pallets /{}", app.search_query)
    } else { "Pallets".into() };

    let items: Vec<ListItem> = app.pallets.iter().map(|p| ListItem::new(p.as_str())).collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title).border_style(panel_border(app, Panel::Pallets)))
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("▸ ");
    let mut state = ListState::default().with_selected(Some(app.pallet_idx));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_storage(f: &mut Frame, app: &App, area: Rect) {
    let pallet_name = app.pallets.get(app.pallet_idx).map(|s| s.as_str()).unwrap_or("");
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let items: Vec<ListItem> = app.storage_items.iter().map(|name| {
        let suffix = app.current_pallet()
            .and_then(|p| p.storage.as_ref())
            .and_then(|s| s.entries.iter().find(|e| e.name == *name))
            .map(|e| match &e.ty {
                StorageEntryType::Plain(ty) => format!(" → {}", types::describe(*ty, &app.meta.registry)),
                StorageEntryType::Map { key, value, .. } => format!(" ({} → {})", types::describe(*key, &app.meta.registry), types::describe(*value, &app.meta.registry)),
            })
            .unwrap_or_default();
        ListItem::new(format!("{name}{suffix}"))
    }).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("{pallet_name} Storage")).border_style(panel_border(app, Panel::Storage)))
        .highlight_style(if app.panel == Panel::Storage { Style::default().bg(Color::DarkGray).bold() } else { Style::default() })
        .highlight_symbol("▸ ");
    let selected = if app.panel == Panel::Storage || app.storage_result.is_some() { Some(app.storage_idx) } else { None };
    let mut state = ListState::default().with_selected(selected);
    f.render_stateful_widget(list, split[0], &mut state);

    let result_area = split[1];
    if matches!(app.focus, Focus::Form) && app.panel == Panel::Storage {
        if let Some(ref form) = app.storage_form {
            let block = Block::default().borders(Borders::ALL).title("Parameters").border_style(Style::default().fg(Color::Yellow));
            let inner = block.inner(result_area);
            f.render_widget(block, result_area);
            form.render(f, inner);
            return;
        }
    }
    f.render_widget(
        Paragraph::new(app.storage_result.as_deref().unwrap_or("")).wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Result").border_style(Style::default().fg(Color::DarkGray))),
        result_area,
    );
}

fn draw_calls(f: &mut Frame, app: &App, area: Rect) {
    let pallet_name = app.pallets.get(app.pallet_idx).map(|s| s.as_str()).unwrap_or("");
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let items: Vec<ListItem> = app.call_items.iter().map(|name| ListItem::new(name.as_str())).collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("{pallet_name} Calls")).border_style(panel_border(app, Panel::Calls)))
        .highlight_style(if app.panel == Panel::Calls { Style::default().bg(Color::DarkGray).bold() } else { Style::default() })
        .highlight_symbol("▸ ");
    let selected = if app.panel == Panel::Calls || app.call_result.is_some() { Some(app.call_idx) } else { None };
    let mut state = ListState::default().with_selected(selected);
    f.render_stateful_widget(list, split[0], &mut state);

    let result_area = split[1];
    if matches!(app.focus, Focus::Form) && app.panel == Panel::Calls {
        if let Some(ref form) = app.call_form {
            let block = Block::default().borders(Borders::ALL).title("Parameters").border_style(Style::default().fg(Color::Yellow));
            let inner = block.inner(result_area);
            f.render_widget(block, result_area);
            form.render(f, inner);
            return;
        }
    }
    f.render_widget(
        Paragraph::new(app.call_result.as_deref().unwrap_or("")).wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Result").border_style(Style::default().fg(Color::DarkGray))),
        result_area,
    );
}

fn draw_blocks(f: &mut Frame, app: &App, area: Rect) {
    if app.recent_blocks.is_empty() {
        f.render_widget(
            Paragraph::new(" waiting for blocks...").block(
                Block::default().borders(Borders::ALL).title("Recent Blocks").border_style(panel_border(app, Panel::Blocks)),
            ),
            area,
        );
        return;
    }

    let inner_width = area.width.saturating_sub(2) as usize;
    let mut spans = Vec::new();
    let mut used = 0;

    for b in app.recent_blocks.iter().rev() {
        let label = if b.has_extrinsics { format!("#{}*", b.number) } else { format!("#{}", b.number) };
        let sep_len = if spans.is_empty() { 0 } else { 2 };
        if used + sep_len + label.len() > inner_width { break; }
        if !spans.is_empty() { spans.push(Span::raw("  ")); used += 2; }
        let color = if b.finalized { Color::Green } else { Color::Yellow };
        spans.push(Span::styled(label.clone(), Style::default().fg(color)));
        used += label.len();
    }
    spans.reverse();

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default().borders(Borders::ALL).title("Recent Blocks").border_style(panel_border(app, Panel::Blocks)),
        ),
        area,
    );
}

fn draw_block_detail_dialog(f: &mut Frame, app: &App, detail: &BlockDetail) {
    let area = f.area();
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    // Block list (newest first)
    let items: Vec<ListItem> = app.recent_blocks.iter().rev().map(|b| {
        let label = if b.has_extrinsics {
            format!("#{} * ({} events)", b.number, b.event_count)
        } else {
            format!("#{}", b.number)
        };
        let color = if b.finalized { Color::Green } else { Color::Yellow };
        ListItem::new(label).style(Style::default().fg(color))
    }).collect();

    let rev_idx = app.recent_blocks.len().saturating_sub(1 + detail.block_idx);
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Blocks").border_style(Style::default().fg(Color::Cyan)))
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("▸ ");
    let mut state = ListState::default().with_selected(Some(rev_idx));
    f.render_stateful_widget(list, layout[0], &mut state);

    // Right side: header + events + help
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3), Constraint::Length(1)])
        .split(layout[1]);

    // Block header
    let block = app.recent_blocks.get(detail.block_idx);
    let header_text = match block {
        Some(b) => {
            let status = if b.finalized { "✓ finalized" } else { "○ pending" };
            let status_color = if b.finalized { Color::Green } else { Color::Yellow };
            vec![
                Line::from(vec![
                    Span::styled(format!("Block #{}", b.number), Style::default().bold().fg(Color::White)),
                    Span::raw("  "),
                    Span::styled(status, Style::default().fg(status_color)),
                ]),
                Line::from(Span::styled(
                    b.hash.to_string(),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(format!("{} events", b.event_count)),
            ]
        }
        None => vec![Line::from("No block selected")],
    };

    f.render_widget(
        Paragraph::new(header_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        right[0],
    );

    // Events
    let events_text = detail.events.as_deref().unwrap_or("");
    f.render_widget(
        Paragraph::new(events_text)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Events")
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        right[1],
    );

    // Help
    f.render_widget(
        Paragraph::new(" ↑↓ navigate blocks  Esc back").fg(Color::DarkGray),
        right[2],
    );
}
