use ratatui::prelude::*;
use ratatui::widgets::*;

use sube::metadata::StorageEntryType;

use super::types;
use super::{App, BlockDetail, Focus, Panel};

pub fn draw(f: &mut Frame, app: &App) {
    if let Some(ref detail) = app.block_detail {
        draw_block_detail(f, app, detail);
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

    let latest = app
        .recent_blocks
        .last()
        .map(|b| format!(" · block #{}", b.number))
        .unwrap_or_default();
    f.render_widget(
        Paragraph::new(format!(" sube · {}{latest}", app.chain_url))
            .bold()
            .fg(Color::Cyan),
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
        (Focus::Form, _) => {
            " ↑↓ fields  ←→ enum  Space toggle  +/- list  Enter next/submit  Esc cancel"
        }
        (Focus::BlockDetail, _) => " ↑↓ navigate  Esc back",
        (_, Panel::Pallets) => " ↑↓ navigate  Tab panel  / filter  q quit",
        (_, Panel::Storage) => " ↑↓ navigate  Tab panel  Enter query  q quit",
        (_, Panel::Calls) => " ↑↓ navigate  Tab panel  Enter select  q quit",
        (_, Panel::Blocks) => " Tab panel  Enter details  q quit",
    };
    if let Some(ref err) = app.error {
        f.render_widget(
            Paragraph::new(format!(" error: {err}")).fg(Color::Red),
            outer[3],
        );
    } else {
        f.render_widget(Paragraph::new(help).fg(Color::DarkGray), outer[3]);
    }
}

fn border(app: &App, panel: Panel) -> Style {
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
                .border_style(border(app, Panel::Pallets)),
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
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

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
                    StorageEntryType::Map { key, value, .. } => format!(
                        " ({} → {})",
                        types::describe(*key, &app.meta.registry),
                        types::describe(*value, &app.meta.registry)
                    ),
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
                .border_style(border(app, Panel::Storage)),
        )
        .highlight_style(if app.panel == Panel::Storage {
            Style::default().bg(Color::DarkGray).bold()
        } else {
            Style::default()
        })
        .highlight_symbol("▸ ");
    let selected = if app.panel == Panel::Storage || app.storage_result.is_some() {
        Some(app.storage_idx)
    } else {
        None
    };
    let mut state = ListState::default().with_selected(selected);
    f.render_stateful_widget(list, split[0], &mut state);

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
    f.render_widget(
        Paragraph::new(app.storage_result.as_deref().unwrap_or(""))
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
                .border_style(border(app, Panel::Calls)),
        )
        .highlight_style(if app.panel == Panel::Calls {
            Style::default().bg(Color::DarkGray).bold()
        } else {
            Style::default()
        })
        .highlight_symbol("▸ ");
    let selected = if app.panel == Panel::Calls || app.call_result.is_some() {
        Some(app.call_idx)
    } else {
        None
    };
    let mut state = ListState::default().with_selected(selected);
    f.render_stateful_widget(list, split[0], &mut state);

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
    f.render_widget(
        Paragraph::new(app.call_result.as_deref().unwrap_or(""))
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
    if app.recent_blocks.is_empty() {
        f.render_widget(
            Paragraph::new(" waiting for blocks...").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Recent Blocks")
                    .border_style(border(app, Panel::Blocks)),
            ),
            area,
        );
        return;
    }

    let inner_width = area.width.saturating_sub(2) as usize;
    let mut spans = Vec::new();
    let mut used = 0;

    for b in app.recent_blocks.iter().rev() {
        let label = if b.has_extrinsics {
            format!("#{}*", b.number)
        } else {
            format!("#{}", b.number)
        };
        let sep_len = if spans.is_empty() { 0 } else { 2 };
        if used + sep_len + label.len() > inner_width {
            break;
        }
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
            used += 2;
        }
        let color = if b.finalized {
            Color::Green
        } else {
            Color::Yellow
        };
        spans.push(Span::styled(label.clone(), Style::default().fg(color)));
        used += label.len();
    }
    spans.reverse();

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recent Blocks")
                .border_style(border(app, Panel::Blocks)),
        ),
        area,
    );
}

fn draw_block_detail(f: &mut Frame, app: &App, detail: &BlockDetail) {
    let area = f.area();
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let items: Vec<ListItem> = app
        .recent_blocks
        .iter()
        .rev()
        .map(|b| {
            let label = if b.has_extrinsics {
                format!("#{} * ({} events)", b.number, b.event_count)
            } else {
                format!("#{}", b.number)
            };
            let color = if b.finalized {
                Color::Green
            } else {
                Color::Yellow
            };
            ListItem::new(label).style(Style::default().fg(color))
        })
        .collect();

    let rev_idx = app
        .recent_blocks
        .len()
        .saturating_sub(1 + detail.block_idx);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Blocks")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("▸ ");
    let mut state = ListState::default().with_selected(Some(rev_idx));
    f.render_stateful_widget(list, layout[0], &mut state);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(layout[1]);

    let block = app.recent_blocks.get(detail.block_idx);
    let header_text = match block {
        Some(b) => {
            let status = if b.finalized {
                "✓ finalized"
            } else {
                "○ pending"
            };
            let status_color = if b.finalized {
                Color::Green
            } else {
                Color::Yellow
            };
            vec![
                Line::from(vec![
                    Span::styled(
                        format!("Block #{}", b.number),
                        Style::default().bold().fg(Color::White),
                    ),
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

    f.render_widget(
        Paragraph::new(detail.events.as_deref().unwrap_or(""))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Events")
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        right[1],
    );

    f.render_widget(
        Paragraph::new(" ↑↓ navigate blocks  Esc back").fg(Color::DarkGray),
        right[2],
    );
}
