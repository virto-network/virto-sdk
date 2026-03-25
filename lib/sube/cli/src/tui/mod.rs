use std::sync::mpsc;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;

use sube::metadata::{PalletMeta, StorageEntryType};
use sube::Metadata;

mod chain;
mod draw;
mod form;
mod format;
mod types;

use chain::{FromChain, ToChain};
use form::{field_from_type, FormState};
use format::fuzzy_match;

// --- Types ---

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

struct BlockInfo {
    number: u64,
    hash: String,
    finalized: bool,
    has_extrinsics: bool,
    event_count: usize,
}

struct BlockDetail {
    block_idx: usize,
    events: Option<String>,
    pending_hash: Option<(String, std::time::Instant)>,
}

enum Focus {
    Navigate,
    Form,
    Search,
    BlockDetail,
}

// --- App state ---

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

        if let Some(calls_ty) = pallet.calls_ty {
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
            let reg = sube::Arc::new(self.meta.registry.clone());
            match &entry.ty {
                StorageEntryType::Plain(_) => {
                    self.storage_form = Some(FormState::new(vec![], reg));
                }
                StorageEntryType::Map { hashers, key, .. } => {
                    let key_types =
                        types::extract_key_types(*key, hashers.len(), &self.meta.registry);
                    let fields = key_types
                        .iter()
                        .enumerate()
                        .map(|(i, ty_id)| {
                            field_from_type(&format!("key{i}"), *ty_id, &self.meta.registry)
                        })
                        .collect();
                    self.storage_form = Some(FormState::new(fields, reg));
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
                            .map(|(i, id)| {
                                field_from_type(&format!("field{i}"), *id, &self.meta.registry)
                            })
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
            FromChain::StorageResult(text) => self.storage_result = Some(text),
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
        let Some(ref mut detail) = self.block_detail else {
            return;
        };
        let new_idx = (detail.block_idx as isize + delta)
            .clamp(0, self.recent_blocks.len() as isize - 1) as usize;
        if new_idx != detail.block_idx {
            detail.block_idx = new_idx;
            detail.events = None;
            let hash = self.recent_blocks[new_idx].hash.clone();
            detail.pending_hash = Some((hash, std::time::Instant::now()));
        }
    }

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

// --- Entry point ---

pub async fn run(chain_url: &str) -> Result<()> {
    eprintln!("Connecting to {chain_url}...");
    let chain = sube::Sube::connect(chain_url).await?;
    eprintln!("Connected, loading metadata...");
    let meta = chain.metadata_arc();

    let (ui_tx, chain_rx) = mpsc::channel::<ToChain>();
    let (chain_tx, ui_rx) = smol::channel::unbounded::<FromChain>();

    chain::spawn(chain, chain_rx, chain_tx);

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
        terminal.draw(|f| draw::draw(f, &app))?;

        while let Ok(msg) = ui_rx.try_recv() {
            app.process_chain_message(msg);
        }
        app.flush_pending_detail();

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
                if key.code == KeyCode::Char('q') && matches!(app.focus, Focus::Navigate) {
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

// --- Input handling ---

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
                if app.storage_idx > 0 {
                    app.storage_idx -= 1;
                }
            }
            Panel::Calls => {
                if app.call_idx > 0 {
                    app.call_idx -= 1;
                }
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
                if app.storage_idx + 1 < app.storage_items.len() {
                    app.storage_idx += 1;
                }
            }
            Panel::Calls => {
                if app.call_idx + 1 < app.call_items.len() {
                    app.call_idx += 1;
                }
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
        _ => {
            app.focus = Focus::Navigate;
            return;
        }
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
        KeyCode::Backspace => {
            app.search_query.pop();
            app.apply_filter();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.apply_filter();
        }
        _ => {}
    }
}

fn handle_block_detail(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.block_detail = None;
            app.focus = Focus::Navigate;
        }
        KeyCode::Up | KeyCode::Char('k') => app.navigate_block_detail(1),
        KeyCode::Down | KeyCode::Char('j') => app.navigate_block_detail(-1),
        _ => {}
    }
}
