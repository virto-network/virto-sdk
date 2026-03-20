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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Storage,
    Constants,
    Calls,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Storage, Tab::Constants, Tab::Calls];

    fn label(&self) -> &str {
        match self {
            Tab::Storage => "Storage",
            Tab::Constants => "Constants",
            Tab::Calls => "Calls",
        }
    }

    fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

enum Focus {
    Pallets,
    Items,
    Form,
    Search,
}

struct App<'a> {
    chain: &'a Sube,
    meta: &'a Metadata,
    all_pallets: Vec<String>,
    pallets: Vec<String>,
    pallet_idx: usize,
    tab: Tab,
    all_items: Vec<String>,
    items: Vec<String>,
    item_idx: usize,
    focus: Focus,
    pre_search_focus: Option<bool>, // true = was on Items, false = was on Pallets
    search_query: String,
    form: Option<FormState>,
    result: Option<String>,
    error: Option<String>,
    should_quit: bool,
}

impl<'a> App<'a> {
    fn new(chain: &'a Sube, meta: &'a Metadata) -> Self {
        let mut all_pallets: Vec<String> = meta.pallets.iter().map(|p| p.name.clone()).collect();
        all_pallets.sort();
        let pallets = all_pallets.clone();
        let mut app = App {
            chain,
            meta,
            all_pallets,
            pallets,
            pallet_idx: 0,
            tab: Tab::Storage,
            all_items: vec![],
            items: vec![],
            item_idx: 0,
            focus: Focus::Pallets,
            pre_search_focus: None,
            search_query: String::new(),
            form: None,
            result: None,
            error: None,
            should_quit: false,
        };
        app.refresh_items();
        app
    }

    fn current_pallet(&self) -> Option<&PalletMeta> {
        let name = self.pallets.get(self.pallet_idx)?;
        self.meta.pallet_by_name(name)
    }

    fn refresh_items(&mut self) {
        self.all_items.clear();
        self.items.clear();
        self.item_idx = 0;
        self.form = None;
        self.result = None;
        self.error = None;

        let Some(pallet) = self.current_pallet() else {
            return;
        };

        match self.tab {
            Tab::Storage => {
                if let Some(storage) = &pallet.storage {
                    self.all_items = storage.entries.iter().map(|e| e.name.clone()).collect();
                }
            }
            Tab::Constants => {
                self.all_items = pallet.constants.iter().map(|c| c.name.clone()).collect();
            }
            Tab::Calls => {
                if let Some(calls_ty) = pallet.calls_ty {
                    if let Some(sube::scales::TypeDef::Variant(vdef)) =
                        self.meta.registry.resolve(calls_ty)
                    {
                        self.all_items = vdef.variants.iter().map(|v| v.name.clone()).collect();
                    }
                }
            }
        }
        self.items = self.all_items.clone();
    }

    fn enter_search(&mut self) {
        self.pre_search_focus = Some(matches!(self.focus, Focus::Items));
        self.search_query.clear();
        self.focus = Focus::Search;
    }

    fn apply_filter(&mut self) {
        let q = self.search_query.to_lowercase();
        if q.is_empty() {
            match self.pre_search_focus {
                Some(true) | None => {
                    self.items = self.all_items.clone();
                }
                Some(false) => {
                    self.pallets = self.all_pallets.clone();
                }
            }
        } else {
            match self.pre_search_focus {
                Some(true) | None => {
                    self.items = self
                        .all_items
                        .iter()
                        .filter(|name| fuzzy_match(&q, name))
                        .cloned()
                        .collect();
                }
                Some(false) => {
                    self.pallets = self
                        .all_pallets
                        .iter()
                        .filter(|name| fuzzy_match(&q, name))
                        .cloned()
                        .collect();
                }
            }
        }
        // Clamp indices
        if self.pallet_idx >= self.pallets.len() {
            self.pallet_idx = self.pallets.len().saturating_sub(1);
        }
        if self.item_idx >= self.items.len() {
            self.item_idx = self.items.len().saturating_sub(1);
        }
    }

    fn exit_search(&mut self) {
        self.focus = match self.pre_search_focus {
            Some(true) => Focus::Items,
            _ => Focus::Pallets,
        };
        self.pre_search_focus = None;
        // Keep the filtered list; user can press / again or navigate
    }

    fn clear_filter(&mut self) {
        self.search_query.clear();
        self.pallets = self.all_pallets.clone();
        self.items = self.all_items.clone();
        if self.pallet_idx >= self.pallets.len() {
            self.pallet_idx = 0;
        }
        if self.item_idx >= self.items.len() {
            self.item_idx = 0;
        }
    }

    fn build_form(&mut self) {
        self.form = None;
        self.result = None;
        self.error = None;

        let pallet = match self.current_pallet() {
            Some(p) => p,
            None => return,
        };
        let item_name = match self.items.get(self.item_idx) {
            Some(n) => n.clone(),
            None => return,
        };

        match self.tab {
            Tab::Storage => {
                let entry = pallet
                    .storage
                    .as_ref()
                    .and_then(|s| s.entries.iter().find(|e| e.name == item_name));
                if let Some(entry) = entry {
                    match &entry.ty {
                        StorageEntryType::Plain(_) => {
                            self.form = Some(FormState::new(vec![]));
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
                                    (format!("key{}", i), desc)
                                })
                                .collect();
                            self.form = Some(FormState::new(fields));
                        }
                    }
                }
            }
            Tab::Constants => {
                self.form = Some(FormState::new(vec![]));
            }
            Tab::Calls => {
                if let Some(calls_ty) = pallet.calls_ty {
                    if let Some(sube::scales::TypeDef::Variant(vdef)) =
                        self.meta.registry.resolve(calls_ty)
                    {
                        if let Some(variant) = vdef.variants.iter().find(|v| v.name == item_name) {
                            let fields = types::variant_fields(variant, &self.meta.registry);
                            self.form = Some(FormState::new(fields));
                        }
                    }
                }
            }
        }
    }

    async fn execute(&mut self) {
        self.error = None;
        self.result = None;

        let pallet = match self.current_pallet() {
            Some(p) => p,
            None => return,
        };
        let item_name = match self.items.get(self.item_idx) {
            Some(n) => n.clone(),
            None => return,
        };

        match self.tab {
            Tab::Storage => {
                let values: Vec<String> = self
                    .form
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
                    Ok(resp) => self.result = Some(format_response(resp)),
                    Err(e) => self.error = Some(format!("{e}")),
                }
            }
            Tab::Constants => {
                let path = format!("{}/_constants/{item_name}", pallet.name);
                match self.chain.query(&path).await {
                    Ok(resp) => self.result = Some(format_response(resp)),
                    Err(e) => self.error = Some(format!("{e}")),
                }
            }
            Tab::Calls => {
                let values: Vec<String> = self
                    .form
                    .as_ref()
                    .map(|f| f.values())
                    .unwrap_or_default();
                let field_names: Vec<String> = self
                    .form
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
                self.result = Some(format!(
                    "sube -c <chain> {}/{} --body '{body}'",
                    pallet.name, item_name,
                ));
            }
        }
    }
}

/// Simple fuzzy match: all characters of the query appear in order in the target.
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
        sube::Response::Value(entry, registry) => entry
            .to_text(registry)
            .unwrap_or_else(|e| format!("error: {e}")),
        sube::Response::ValueSet(items, registry) => {
            let mut out = String::new();
            for (keys, value) in items {
                let key_strs: Vec<String> = keys
                    .iter()
                    .filter_map(|k| k.to_text(registry).ok())
                    .collect();
                let key_display = key_strs.join(", ");
                match value {
                    Some(v) => {
                        let text = v.to_text(registry).unwrap_or_else(|e| format!("error: {e}"));
                        out.push_str(&format!("[{key_display}] {text}\n"));
                    }
                    None => out.push_str(&format!("[{key_display}] (none)\n")),
                }
            }
            out
        }
        sube::Response::Void => "(void)".into(),
        sube::Response::Meta(_) => "(metadata)".into(),
        sube::Response::Registry(_) => "(registry)".into(),
    }
}

pub async fn run(chain_url: &str) -> Result<()> {
    eprintln!("Connecting to {chain_url}...");
    let chain = sube::Sube::connect(chain_url).await?;
    eprintln!("Connected, loading metadata...");
    let meta = chain.metadata();

    enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let mut app = App::new(&chain, meta);

    loop {
        terminal.draw(|f| draw(f, &app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Global quit
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    app.should_quit = true;
                }
                if key.code == KeyCode::Char('q')
                    && !matches!(app.focus, Focus::Form | Focus::Search)
                {
                    app.should_quit = true;
                }

                match app.focus {
                    Focus::Pallets => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.pallet_idx > 0 {
                                app.pallet_idx -= 1;
                                app.refresh_items();
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.pallet_idx + 1 < app.pallets.len() {
                                app.pallet_idx += 1;
                                app.refresh_items();
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                            if !app.items.is_empty() {
                                app.focus = Focus::Items;
                            }
                        }
                        KeyCode::Tab => {
                            app.tab = app.tab.next();
                            app.clear_filter();
                            app.refresh_items();
                        }
                        KeyCode::BackTab => {
                            app.tab = app.tab.prev();
                            app.clear_filter();
                            app.refresh_items();
                        }
                        KeyCode::Char('/') => app.enter_search(),
                        _ => {}
                    },
                    Focus::Items => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.item_idx > 0 {
                                app.item_idx -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.item_idx + 1 < app.items.len() {
                                app.item_idx += 1;
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') | KeyCode::Esc => {
                            app.focus = Focus::Pallets;
                            app.clear_filter();
                            app.result = None;
                            app.error = None;
                        }
                        KeyCode::Tab => {
                            app.tab = app.tab.next();
                            app.clear_filter();
                            app.refresh_items();
                        }
                        KeyCode::BackTab => {
                            app.tab = app.tab.prev();
                            app.clear_filter();
                            app.refresh_items();
                        }
                        KeyCode::Enter => {
                            app.build_form();
                            if let Some(ref f) = app.form {
                                if f.is_empty() {
                                    app.execute().await;
                                } else {
                                    app.focus = Focus::Form;
                                }
                            }
                        }
                        KeyCode::Char('/') => app.enter_search(),
                        _ => {}
                    },
                    Focus::Form => {
                        if let Some(ref mut form) = app.form {
                            match key.code {
                                KeyCode::Esc => {
                                    app.focus = Focus::Items;
                                    app.form = None;
                                    app.result = None;
                                }
                                KeyCode::Enter => {
                                    if form.is_on_last() {
                                        app.execute().await;
                                    } else {
                                        form.next_field();
                                    }
                                }
                                KeyCode::Up => form.prev_field(),
                                KeyCode::Down | KeyCode::Tab => form.next_field(),
                                KeyCode::BackTab => form.prev_field(),
                                KeyCode::Backspace => form.backspace(),
                                KeyCode::Char(c) => form.push_char(c),
                                _ => {}
                            }
                        }
                    }
                    Focus::Search => match key.code {
                        KeyCode::Esc => {
                            app.clear_filter();
                            app.exit_search();
                        }
                        KeyCode::Enter => {
                            app.exit_search();
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
                    },
                }
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

fn draw(f: &mut Frame, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Length(1), // tabs
            Constraint::Min(5),   // body
            Constraint::Length(3), // result/status
        ])
        .split(f.area());

    // Title
    f.render_widget(
        Paragraph::new("sube explorer").bold().centered(),
        outer[0],
    );

    // Tabs
    let tab_titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::from(t.label())).collect();
    let tabs = Tabs::new(tab_titles)
        .select(Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0))
        .highlight_style(Style::default().fg(Color::Cyan).bold());
    f.render_widget(tabs, outer[1]);

    // Body: pallets | items + form
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(outer[2]);

    // Pallet list
    let pallet_items: Vec<ListItem> = app
        .pallets
        .iter()
        .map(|p| ListItem::new(p.as_str()))
        .collect();
    let pallet_title = if matches!(app.focus, Focus::Search)
        && matches!(app.pre_search_focus, Some(false))
    {
        format!("Pallets /{}", app.search_query)
    } else {
        "Pallets".into()
    };
    let pallet_list = List::new(pallet_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(pallet_title)
                .border_style(
                    if matches!(app.focus, Focus::Pallets)
                        || (matches!(app.focus, Focus::Search)
                            && matches!(app.pre_search_focus, Some(false)))
                    {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default()
                    },
                ),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("> ");
    let mut pallet_state = ListState::default().with_selected(Some(app.pallet_idx));
    f.render_stateful_widget(pallet_list, body[0], &mut pallet_state);

    // Right side: items + optional form
    let right = if app.form.is_some() && matches!(app.focus, Focus::Form) {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(body[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)])
            .split(body[1])
    };

    // Item list
    let item_items: Vec<ListItem> = app
        .items
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let suffix = match app.tab {
                Tab::Storage => {
                    let pallet = app.current_pallet();
                    pallet
                        .and_then(|p| p.storage.as_ref())
                        .and_then(|s| s.entries.iter().find(|e| e.name == *name))
                        .map(|e| match &e.ty {
                            StorageEntryType::Plain(ty) => {
                                format!(" -> {}", types::describe(*ty, &app.meta.registry))
                            }
                            StorageEntryType::Map { key, value, .. } => {
                                format!(
                                    " ({} -> {})",
                                    types::describe(*key, &app.meta.registry),
                                    types::describe(*value, &app.meta.registry)
                                )
                            }
                        })
                        .unwrap_or_default()
                }
                Tab::Constants => {
                    let pallet = app.current_pallet();
                    pallet
                        .and_then(|p| p.constants.iter().find(|c| c.name == *name))
                        .map(|c| format!(": {}", types::describe(c.ty, &app.meta.registry)))
                        .unwrap_or_default()
                }
                Tab::Calls => String::new(),
            };
            let style = if i == app.item_idx && !matches!(app.focus, Focus::Pallets) {
                Style::default().bg(Color::DarkGray).bold()
            } else {
                Style::default()
            };
            ListItem::new(format!("{name}{suffix}")).style(style)
        })
        .collect();

    let items_title = if matches!(app.focus, Focus::Search)
        && matches!(app.pre_search_focus, Some(true) | None)
    {
        format!("{} /{}", app.tab.label(), app.search_query)
    } else {
        app.tab.label().into()
    };
    let items_block = Block::default()
        .borders(Borders::ALL)
        .title(items_title)
        .border_style(
            if matches!(app.focus, Focus::Items)
                || (matches!(app.focus, Focus::Search)
                    && matches!(app.pre_search_focus, Some(true) | None))
            {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            },
        );
    let items_list = List::new(item_items)
        .block(items_block)
        .highlight_symbol("> ");
    let mut item_state = ListState::default().with_selected(
        if matches!(app.focus, Focus::Pallets) {
            None
        } else {
            Some(app.item_idx)
        },
    );
    f.render_stateful_widget(items_list, right[0], &mut item_state);

    // Form
    if let (Some(form), true) = (&app.form, right.len() > 1) {
        let form_block = Block::default()
            .borders(Borders::ALL)
            .title("Parameters")
            .border_style(if matches!(app.focus, Focus::Form) {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });
        let inner = form_block.inner(right[1]);
        f.render_widget(form_block, right[1]);
        form.render(f, inner);
    }

    // Result/status bar
    let status_text = if let Some(ref err) = app.error {
        Paragraph::new(err.as_str())
            .style(Style::default().fg(Color::Red))
            .block(Block::default().borders(Borders::ALL).title("Error"))
    } else if let Some(ref res) = app.result {
        Paragraph::new(res.as_str())
            .block(Block::default().borders(Borders::ALL).title("Result"))
    } else {
        let help = match app.focus {
            Focus::Pallets => {
                "hjkl navigate  Tab/S-Tab switch  / filter  Enter select  q quit"
            }
            Focus::Items => {
                "hjkl navigate  Tab/S-Tab switch  / filter  Enter select  Esc back  q quit"
            }
            Focus::Form => "Up/Down fields  Tab/S-Tab fields  Enter next/submit  Esc cancel",
            Focus::Search => "type to filter  Enter confirm  Esc clear & cancel",
        };
        Paragraph::new(help)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL))
    };
    f.render_widget(status_text, outer[3]);
}
