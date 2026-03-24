use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use ratatui::widgets::*;

use sube::metadata::{PalletMeta, StorageEntryType};
use sube::{Metadata, Sube};

mod form;
mod types;

use form::FormState;

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

// --- Block info ---

struct BlockInfo {
    number: u64,
    hash: String,
}

// --- App state ---

enum Focus {
    Navigate,
    Form,
    Search,
}

struct App<'a> {
    chain: &'a mut Sube,
    chain_url: String,
    meta: sube::Arc<Metadata>,

    // Pallets
    all_pallets: Vec<String>,
    pallets: Vec<String>,
    pallet_idx: usize,

    // Storage items
    storage_items: Vec<String>,
    storage_idx: usize,
    storage_form: Option<FormState>,
    storage_result: Option<String>,

    // Call items
    call_items: Vec<String>,
    call_idx: usize,
    call_form: Option<FormState>,
    call_result: Option<String>,

    // Active panel and focus
    panel: Panel,
    focus: Focus,

    // Search
    search_query: String,

    // Mini explorer
    recent_blocks: Vec<BlockInfo>,

    // Status
    error: Option<String>,
    should_quit: bool,
}

impl<'a> App<'a> {
    fn new(chain: &'a mut Sube, chain_url: String, meta: sube::Arc<Metadata>) -> Self {
        let mut all_pallets: Vec<String> = meta.pallets.iter().map(|p| p.name.clone()).collect();
        all_pallets.sort();
        let pallets = all_pallets.clone();
        let mut app = App {
            chain,
            chain_url,
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
            error: None,
            should_quit: false,
        };
        app.refresh_pallet();
        app
    }

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

        // Storage items
        if let Some(storage) = &pallet.storage {
            self.storage_items = storage.entries.iter().map(|e| e.name.clone()).collect();
        }

        // Call variants
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
            match &entry.ty {
                StorageEntryType::Plain(_) => {
                    self.storage_form = Some(FormState::new(vec![]));
                }
                StorageEntryType::Map {
                    hashers,
                    key,
                    value: _,
                } => {
                    let key_types =
                        types::extract_key_types(*key, hashers.len(), &self.meta.registry);
                    let fields: Vec<(String, String)> = key_types
                        .iter()
                        .enumerate()
                        .map(|(i, ty_id)| {
                            let desc = types::describe(*ty_id, &self.meta.registry);
                            (format!("key{i}"), desc)
                        })
                        .collect();
                    self.storage_form = Some(FormState::new(fields));
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
                    let fields = types::variant_fields(variant, &self.meta.registry);
                    self.call_form = Some(FormState::new(fields));
                }
            }
        }
    }

    async fn execute_storage_query(&mut self) {
        self.error = None;
        self.storage_result = None;

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
        match self.chain.query(&path).await {
            Ok(resp) => self.storage_result = Some(format_response(resp)),
            Err(e) => self.error = Some(format!("{e}")),
        }
    }

    async fn execute_call(&mut self) {
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

    async fn record_block(&mut self, event: sube::ChainEvent) {
        if let sube::ChainEvent::NewBlock { hash, .. } = event {
            let number = self
                .chain
                .header(&hash)
                .await
                .map(|h| h.number)
                .unwrap_or(0);
            self.recent_blocks.push(BlockInfo { number, hash });
            if self.recent_blocks.len() > 50 {
                self.recent_blocks.remove(0);
            }
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

// --- Event loop ---

pub async fn run(chain_url: &str) -> Result<()> {
    eprintln!("Connecting to {chain_url}...");
    let mut chain = sube::Sube::connect(chain_url).await?;
    eprintln!("Connected, loading metadata...");
    let meta = chain.metadata_arc();

    enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let mut app = App::new(&mut chain, chain_url.into(), meta);

    loop {
        terminal.draw(|f| draw(f, &app))?;

        // Read terminal input in a thread so we can race it against chain events
        enum Tick {
            Key(crossterm::event::KeyEvent),
            Block(sube::ChainEvent),
            None,
        }

        let tick = {
            use core::pin::pin;
            let chain = &mut *app.chain;

            let term = pin!(async {
                smol::unblock(|| {
                    if event::poll(std::time::Duration::from_secs(1)).unwrap_or(false) {
                        if let Ok(Event::Key(key)) = event::read() {
                            if key.kind == KeyEventKind::Press {
                                return Tick::Key(key);
                            }
                        }
                    }
                    Tick::None
                })
                .await
            });

            let chain_ev = pin!(async {
                match chain.next_event().await {
                    Ok(ev) => Tick::Block(ev),
                    Err(_) => Tick::None,
                }
            });

            smol::future::or(term, chain_ev).await
        };

        match tick {
            Tick::Key(key) => {
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    app.should_quit = true;
                }
                if key.code == KeyCode::Char('q') && matches!(app.focus, Focus::Navigate) {
                    app.should_quit = true;
                }
                match app.focus {
                    Focus::Navigate => handle_navigate(&mut app, key.code).await,
                    Focus::Form => handle_form(&mut app, key.code).await,
                    Focus::Search => handle_search(&mut app, key.code),
                }
            }
            Tick::Block(ev) => {
                app.record_block(ev).await;
            }
            Tick::None => {}
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

async fn handle_navigate(app: &mut App<'_>, key: KeyCode) {
    match key {
        // Panel switching
        KeyCode::Tab => {
            app.panel = app.panel.next();
        }
        KeyCode::BackTab => {
            app.panel = app.panel.prev();
        }

        // Vertical navigation
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

        // Horizontal panel movement
        KeyCode::Right | KeyCode::Char('l') => {
            app.panel = app.panel.next();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.panel = app.panel.prev();
        }

        // Enter: build form or execute
        KeyCode::Enter => match app.panel {
            Panel::Storage => {
                app.build_storage_form();
                if let Some(ref f) = app.storage_form {
                    if f.is_empty() {
                        app.execute_storage_query().await;
                    } else {
                        app.focus = Focus::Form;
                    }
                }
            }
            Panel::Calls => {
                app.build_call_form();
                if let Some(ref f) = app.call_form {
                    if f.is_empty() {
                        app.execute_call().await;
                    } else {
                        app.focus = Focus::Form;
                    }
                }
            }
            _ => {}
        },

        KeyCode::Char('/') => {
            app.search_query.clear();
            app.focus = Focus::Search;
        }
        _ => {}
    }
}

async fn handle_form(app: &mut App<'_>, key: KeyCode) {
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
                    Panel::Storage => app.execute_storage_query().await,
                    Panel::Calls => app.execute_call().await,
                    _ => {}
                }
                app.focus = Focus::Navigate;
            } else {
                form.next_field();
            }
        }
        KeyCode::Up | KeyCode::BackTab => form.prev_field(),
        KeyCode::Down | KeyCode::Tab => form.next_field(),
        KeyCode::Backspace => form.backspace(),
        KeyCode::Char(c) => form.push_char(c),
        _ => {}
    }
}

fn handle_search(app: &mut App<'_>, key: KeyCode) {
    match key {
        KeyCode::Esc => {
            app.search_query.clear();
            app.pallets = app.all_pallets.clone();
            app.focus = Focus::Navigate;
        }
        KeyCode::Enter => {
            app.focus = Focus::Navigate;
        }
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

// --- Drawing ---

fn draw(f: &mut Frame, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(5),   // body
            Constraint::Length(3), // blocks bar
            Constraint::Length(1), // help
        ])
        .split(f.area());

    // Title bar
    let latest_block = app
        .recent_blocks
        .last()
        .map(|b| format!(" · block #{}", b.number))
        .unwrap_or_default();
    f.render_widget(
        Paragraph::new(format!(" sube · {}{latest_block}", app.chain_url))
            .bold()
            .fg(Color::Cyan),
        outer[0],
    );

    // Body: pallets | storage | calls
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

    // Blocks bar
    draw_blocks(f, app, outer[2]);

    // Help / error
    let help_area = outer[3];
    if let Some(ref err) = app.error {
        f.render_widget(
            Paragraph::new(format!(" error: {err}")).fg(Color::Red),
            help_area,
        );
    } else {
        let help = match (&app.focus, &app.panel) {
            (Focus::Search, _) => " / filter  Enter confirm  Esc cancel",
            (Focus::Form, _) => " ↑↓ fields  Enter next/submit  Esc cancel",
            (_, Panel::Pallets) => " ↑↓ navigate  Tab panel  / filter  → select  q quit",
            (_, Panel::Storage) => " ↑↓ navigate  Tab panel  Enter query  q quit",
            (_, Panel::Calls) => " ↑↓ navigate  Tab panel  Enter select  q quit",
            (_, Panel::Blocks) => " Tab panel  q quit",
        };
        f.render_widget(Paragraph::new(help).fg(Color::DarkGray), help_area);
    }
}

fn panel_border(app: &App, panel: Panel) -> Style {
    if app.panel == panel {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn draw_pallets(f: &mut Frame, app: &App, area: Rect) {
    let title = if matches!(app.focus, Focus::Search) {
        format!("Pallets /{}", app.search_query)
    } else {
        "Pallets".into()
    };

    let items: Vec<ListItem> = app
        .pallets
        .iter()
        .map(|p| ListItem::new(p.as_str()))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(panel_border(app, Panel::Pallets)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("▸ ");

    let mut state = ListState::default().with_selected(Some(app.pallet_idx));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_storage(f: &mut Frame, app: &App, area: Rect) {
    let pallet_name = app
        .pallets
        .get(app.pallet_idx)
        .map(|s| s.as_str())
        .unwrap_or("");

    // Split: items list + result/form area
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Storage items list
    let items: Vec<ListItem> = app
        .storage_items
        .iter()
        .map(|name| {
            let suffix = app
                .current_pallet()
                .and_then(|p| p.storage.as_ref())
                .and_then(|s| s.entries.iter().find(|e| e.name == *name))
                .map(|e| match &e.ty {
                    StorageEntryType::Plain(ty) => {
                        format!(" → {}", types::describe(*ty, &app.meta.registry))
                    }
                    StorageEntryType::Map { key, value, .. } => {
                        format!(
                            " ({} → {})",
                            types::describe(*key, &app.meta.registry),
                            types::describe(*value, &app.meta.registry)
                        )
                    }
                })
                .unwrap_or_default();
            ListItem::new(format!("{name}{suffix}"))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("{pallet_name} Storage"))
                .border_style(panel_border(app, Panel::Storage)),
        )
        .highlight_style(
            if app.panel == Panel::Storage {
                Style::default().bg(Color::DarkGray).bold()
            } else {
                Style::default()
            },
        )
        .highlight_symbol("▸ ");

    let selected = if app.panel == Panel::Storage || app.storage_result.is_some() {
        Some(app.storage_idx)
    } else {
        None
    };
    let mut state = ListState::default().with_selected(selected);
    f.render_stateful_widget(list, split[0], &mut state);

    // Result or form
    let result_area = split[1];
    if matches!(app.focus, Focus::Form) && app.panel == Panel::Storage {
        if let Some(ref form) = app.storage_form {
            let block = Block::default()
                .borders(Borders::ALL)
                .title("Parameters")
                .border_style(Style::default().fg(Color::Yellow));
            let inner = block.inner(result_area);
            f.render_widget(block, result_area);
            form.render(f, inner);
            return;
        }
    }

    let content = app
        .storage_result
        .as_deref()
        .unwrap_or("");
    f.render_widget(
        Paragraph::new(content)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Result")
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        result_area,
    );
}

fn draw_calls(f: &mut Frame, app: &App, area: Rect) {
    let pallet_name = app
        .pallets
        .get(app.pallet_idx)
        .map(|s| s.as_str())
        .unwrap_or("");

    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Call items list
    let items: Vec<ListItem> = app
        .call_items
        .iter()
        .map(|name| ListItem::new(name.as_str()))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("{pallet_name} Calls"))
                .border_style(panel_border(app, Panel::Calls)),
        )
        .highlight_style(
            if app.panel == Panel::Calls {
                Style::default().bg(Color::DarkGray).bold()
            } else {
                Style::default()
            },
        )
        .highlight_symbol("▸ ");

    let selected = if app.panel == Panel::Calls || app.call_result.is_some() {
        Some(app.call_idx)
    } else {
        None
    };
    let mut state = ListState::default().with_selected(selected);
    f.render_stateful_widget(list, split[0], &mut state);

    // Result or form
    let result_area = split[1];
    if matches!(app.focus, Focus::Form) && app.panel == Panel::Calls {
        if let Some(ref form) = app.call_form {
            let block = Block::default()
                .borders(Borders::ALL)
                .title("Parameters")
                .border_style(Style::default().fg(Color::Yellow));
            let inner = block.inner(result_area);
            f.render_widget(block, result_area);
            form.render(f, inner);
            return;
        }
    }

    let content = app.call_result.as_deref().unwrap_or("");
    f.render_widget(
        Paragraph::new(content)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Result")
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        result_area,
    );
}

fn draw_blocks(f: &mut Frame, app: &App, area: Rect) {
    let block_text = if app.recent_blocks.is_empty() {
        " waiting for blocks...".into()
    } else {
        let width = area.width as usize;
        let mut parts = Vec::new();
        for b in app.recent_blocks.iter().rev() {
            let s = format!("#{}", b.number);
            parts.push(s);
        }
        let joined = parts.join("  ");
        if joined.len() > width.saturating_sub(4) {
            format!(" …{}", &joined[joined.len().saturating_sub(width.saturating_sub(4))..])
        } else {
            format!(" {joined}")
        }
    };

    f.render_widget(
        Paragraph::new(block_text).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recent Blocks")
                .border_style(panel_border(app, Panel::Blocks)),
        ),
        area,
    );
}
