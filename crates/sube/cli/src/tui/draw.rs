use ratatui::prelude::*;
use ratatui::widgets::*;

use sube::metadata::StorageEntryType;

use super::types;
use super::{App, BlockDetail, Focus, Panel};

pub fn draw(f: &mut Frame, app: &App) {
    if matches!(app.focus, Focus::Profiles) {
        draw_profiles(f, app);
        return;
    }
    #[cfg(feature = "pass")]
    if matches!(app.focus, Focus::PassSession) {
        draw_pass_session(f, app);
        return;
    }
    #[cfg(feature = "pass")]
    if matches!(app.focus, Focus::ConfirmForgetSession) {
        draw_forget_session_confirmation(f, app);
        return;
    }
    #[cfg(feature = "pass")]
    if matches!(app.focus, Focus::PassEnrollment) {
        draw_pass_enrollment(f, app);
        return;
    }
    #[cfg(feature = "pass")]
    if matches!(app.focus, Focus::PassDeviceAddition) {
        draw_pass_device_addition(f, app);
        return;
    }
    #[cfg(feature = "pass")]
    if matches!(app.focus, Focus::PassDeviceRemoval) {
        draw_pass_device_removal(f, app);
        return;
    }
    #[cfg(feature = "wallet")]
    if matches!(app.focus, Focus::WalletImport) {
        draw_wallet_import(f, app);
        return;
    }
    if matches!(app.focus, Focus::Review | Focus::ConfirmSubmit) {
        draw_review(f, app);
        if matches!(app.focus, Focus::ConfirmSubmit) {
            draw_submit_confirmation(f, app);
        }
        return;
    }
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
        (Focus::BodyInput, _) => " Enter review  Esc cancel  Type JSON/text or a file path",
        (Focus::Profiles, _) => unreachable!(),
        #[cfg(feature = "wallet")]
        (Focus::WalletImport, _) => unreachable!(),
        #[cfg(feature = "pass")]
        (Focus::PassSession, _) => unreachable!(),
        #[cfg(feature = "pass")]
        (Focus::ConfirmForgetSession, _) => unreachable!(),
        #[cfg(feature = "pass")]
        (Focus::PassEnrollment, _) => unreachable!(),
        #[cfg(feature = "pass")]
        (Focus::PassDeviceAddition, _) => unreachable!(),
        #[cfg(feature = "pass")]
        (Focus::PassDeviceRemoval, _) => unreachable!(),
        (Focus::BlockDetail, _) => " ↑↓ navigate  Esc back",
        (Focus::Review | Focus::ConfirmSubmit, _) => unreachable!(),
        (_, Panel::Pallets) => " ↑↓ navigate  Tab panel  / filter  p profiles  q quit",
        (_, Panel::Storage) => " ↑↓ navigate  Tab panel  Enter query  q quit",
        (_, Panel::Calls) => " ↑↓ navigate  Tab panel  m input mode  Enter select  q quit",
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

fn draw_profiles(f: &mut Frame, app: &App) {
    let area = centered_rect(88, 24, f.area());
    f.render_widget(Clear, area);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);
    f.render_widget(
        Paragraph::new(format!(
            "Connected genesis: 0x{}",
            hex::encode(app.genesis_hash)
        ))
        .block(Block::default().borders(Borders::ALL).title("Profiles")),
        layout[0],
    );
    let items = app
        .profiles
        .profiles
        .iter()
        .map(|profile| {
            let active = if app.profiles.active.as_deref() == Some(profile.name()) {
                "*"
            } else {
                " "
            };
            let compatible = if profile.genesis_hash() == app.genesis_hash {
                ""
            } else {
                " [GENESIS MISMATCH]"
            };
            #[allow(unreachable_patterns)]
            let kind = match profile {
                crate::profiles::Profile::Wallet(_) => "wallet",
                #[cfg(feature = "pass")]
                crate::profiles::Profile::Pass(profile) => {
                    if profile.session.is_some() {
                        "pass/session"
                    } else {
                        "pass/no-session"
                    }
                }
                _ => "profile",
            };
            ListItem::new(format!("{active} {} ({kind}){compatible}", profile.name()))
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Chain-bound profiles"),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("▸ ");
    let selected = (!app.profiles.profiles.is_empty()).then_some(app.profile_idx);
    let mut state = ListState::default().with_selected(selected);
    f.render_stateful_widget(list, layout[1], &mut state);
    let message = app.error.as_deref().unwrap_or(
        "↑↓ Select  Enter Connect  a Wallet  e Enroll  d Add device  r Remove  s Session  f Forget  Esc Close",
    );
    f.render_widget(
        Paragraph::new(message).wrap(Wrap { trim: false }),
        layout[2],
    );
}

#[cfg(feature = "pass")]
fn draw_pass_enrollment(f: &mut Frame, app: &App) {
    let area = centered_rect(100, 20, f.area());
    f.render_widget(Clear, area);
    let Some(enrollment) = app.pass_enrollment.as_ref() else {
        return;
    };
    let style = |field| {
        if enrollment.field == field {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default()
        }
    };
    let (primary_label, secondary_label) = match enrollment.provider {
        super::DeviceProviderKind::SubstrateKey => ("Device wallet profile", "Unused"),
        #[cfg(feature = "desktop-webauthn")]
        super::DeviceProviderKind::WebAuthn => ("WebAuthn RP ID", "WebAuthn origin"),
        #[cfg(all(feature = "ssh-agent", unix))]
        super::DeviceProviderKind::SshAgent => ("SSH fingerprint", "SSHSIG namespace"),
    };
    let lines = vec![
        Line::styled(format!("Pass profile name: {}", enrollment.name), style(0)),
        Line::styled(
            format!("Hashed user ID (exact 32-byte hex): {}", enrollment.user_id),
            style(1),
        ),
        Line::styled(
            format!("Registrar wallet profile: {}", enrollment.registrar),
            style(2),
        ),
        Line::styled(format!("Provider: {:?}", enrollment.provider), style(3)),
        Line::styled(
            format!("{primary_label}: {}", enrollment.provider_primary),
            style(4),
        ),
        Line::styled(
            format!("{secondary_label}: {}", enrollment.provider_secondary),
            style(5),
        ),
        Line::raw(""),
        Line::raw("←→/Space provider  Tab fields  Enter continue  Esc cancel"),
        Line::raw(
            "The predicted pass address and complete transaction are shown before submission.",
        ),
        Line::styled(
            app.error.as_deref().unwrap_or(""),
            Style::default().fg(Color::Red),
        ),
    ];
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Enroll pass account"),
        ),
        area,
    );
}

#[cfg(feature = "pass")]
fn draw_pass_device_addition(f: &mut Frame, app: &App) {
    let area = centered_rect(100, 19, f.area());
    f.render_widget(Clear, area);
    let Some(device) = app.pass_device_addition.as_ref() else {
        return;
    };
    let style = |field| {
        if device.field == field {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default()
        }
    };
    let (primary_label, secondary_label) = match device.provider {
        super::DeviceProviderKind::SubstrateKey => ("Device wallet profile", "Unused"),
        #[cfg(feature = "desktop-webauthn")]
        super::DeviceProviderKind::WebAuthn => ("WebAuthn RP ID", "WebAuthn origin"),
        #[cfg(all(feature = "ssh-agent", unix))]
        super::DeviceProviderKind::SshAgent => ("SSH fingerprint", "SSHSIG namespace"),
    };
    let lines = vec![
        Line::raw(format!("Pass profile: {}", device.profile)),
        Line::styled(format!("Provider: {:?}", device.provider), style(0)),
        Line::styled(
            format!("{primary_label}: {}", device.provider_primary),
            style(1),
        ),
        Line::styled(
            format!("{secondary_label}: {}", device.provider_secondary),
            style(2),
        ),
        Line::styled(
            format!(
                "Filter (calls:..., pallets:..., or admin): {}",
                device.filter
            ),
            style(3),
        ),
        Line::styled(
            format!(
                "Admin confirmation (type ADMIN): {}",
                device.admin_confirmation
            ),
            style(4),
        ),
        Line::raw(""),
        Line::raw("←→/Space provider  Tab fields  Enter continue  Esc cancel"),
        Line::raw("Adding a device is authenticated directly by the profile's primary device."),
        Line::styled(
            app.error.as_deref().unwrap_or(""),
            Style::default().fg(Color::Red),
        ),
    ];
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Add pass device"),
        ),
        area,
    );
}

#[cfg(feature = "pass")]
fn draw_pass_device_removal(f: &mut Frame, app: &App) {
    let area = centered_rect(100, 22, f.area());
    f.render_widget(Clear, area);
    let Some(removal) = app.pass_device_removal.as_ref() else {
        return;
    };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(8),
        ])
        .split(area);
    f.render_widget(
        Paragraph::new(format!("Pass profile: {}", removal.profile)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Remove pass device"),
        ),
        layout[0],
    );
    let items = removal
        .devices
        .iter()
        .map(|(device_id, label, _)| {
            ListItem::new(format!("{label} · 0x{}", hex::encode(device_id)))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(removal.device_idx));
    f.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Known devices"),
            )
            .highlight_style(Style::default().bg(Color::DarkGray).bold())
            .highlight_symbol("▸ "),
        layout[1],
        &mut state,
    );
    let selected_primary = removal
        .devices
        .get(removal.device_idx)
        .map(|(_, _, primary)| *primary)
        .unwrap_or(false);
    let warning = if selected_primary && removal.devices.len() == 1 {
        "WARNING: this is the last locally known usable device and may also be the last Admin device. The local profile can become unusable."
    } else if selected_primary {
        "WARNING: removing the primary device promotes the first locally recorded additional device. Verify it has a usable/Admin on-chain filter."
    } else {
        "Verify another usable and Admin device remains on-chain; local profiles do not retain authoritative on-chain filter counts."
    };
    f.render_widget(
        Paragraph::new(format!(
            "{warning}\n\nType REMOVE: {}\n↑↓ Select  Enter review  Esc cancel\n{}",
            removal.confirmation,
            app.error.as_deref().unwrap_or("")
        ))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        layout[2],
    );
}

#[cfg(feature = "pass")]
fn draw_forget_session_confirmation(f: &mut Frame, app: &App) {
    let area = centered_rect(86, 10, f.area());
    f.render_widget(Clear, area);
    let profile = app.forget_session_profile.as_deref().unwrap_or("pass");
    f.render_widget(
        Paragraph::new(format!(
            "Forget the local session for {profile}?\n\n{}\n\nEnter/y Confirm   Esc/n Cancel",
            pass::session::FORGET_SESSION_WARNING
        ))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Forget local session")
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        area,
    );
}

#[cfg(feature = "pass")]
fn draw_pass_session(f: &mut Frame, app: &App) {
    let area = centered_rect(94, 14, f.area());
    f.render_widget(Clear, area);
    let Some(session) = app.pass_session.as_ref() else {
        return;
    };
    let policy_style = if session.field == 0 {
        Style::default().fg(Color::Yellow).bold()
    } else {
        Style::default()
    };
    let duration_style = if session.field == 1 {
        Style::default().fg(Color::Yellow).bold()
    } else {
        Style::default()
    };
    let duration = if session.duration.is_empty() {
        "runtime maximum"
    } else {
        &session.duration
    };
    let lines = vec![
        Line::raw(format!("Pass profile: {}", session.profile)),
        Line::styled(format!("Policy: {}", session.policy), policy_style),
        Line::styled(format!("Duration: {duration}"), duration_style),
        Line::raw(""),
        Line::raw(
            "Policy: calls:pallet/call,... | pallets:pallet,... | spend:pallet:limit:call,...",
        ),
        Line::raw("t This call  l This pallet  Tab fields  Enter continue  Esc cancel"),
        Line::raw("Exact on-chain sessions are reused; updates always open transaction review."),
        Line::styled(
            app.error.as_deref().unwrap_or(""),
            Style::default().fg(Color::Red),
        ),
    ];
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Connect pass session"),
        ),
        area,
    );
}

#[cfg(feature = "wallet")]
fn draw_wallet_import(f: &mut Frame, app: &App) {
    let area = centered_rect(88, 12, f.area());
    f.render_widget(Clear, area);
    let Some(import) = app.wallet_import.as_ref() else {
        return;
    };
    let name_style = if import.field == 0 {
        Style::default().fg(Color::Yellow).bold()
    } else {
        Style::default()
    };
    let mnemonic_style = if import.field == 1 {
        Style::default().fg(Color::Yellow).bold()
    } else {
        Style::default()
    };
    let masked = "•".repeat(import.mnemonic.chars().count());
    let lines = vec![
        Line::styled(format!("Profile name: {}", import.name), name_style),
        Line::styled(format!("Mnemonic: {masked}"), mnemonic_style),
        Line::raw(""),
        Line::raw("The //default Sr25519 account is derived and verified before storage."),
        Line::raw("Tab fields  Enter import  Esc cancel"),
        Line::styled(
            app.error.as_deref().unwrap_or(""),
            Style::default().fg(Color::Red),
        ),
    ];
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Import wallet profile"),
        ),
        area,
    );
}

fn draw_review(f: &mut Frame, app: &App) {
    let area = f.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);
    f.render_widget(
        Paragraph::new(format!(" Transaction review · wait for {:?}", app.wait_for))
            .bold()
            .fg(Color::Cyan),
        layout[0],
    );
    f.render_widget(
        Paragraph::new(app.call_result.as_deref().unwrap_or("Preparing review..."))
            .wrap(Wrap { trim: false })
            .scroll((app.review_scroll, 0))
            .block(Block::default().borders(Borders::ALL).title("Review")),
        layout[1],
    );
    let help = if app.call_submittable {
        if app.review_requires_finalized {
            " Esc Back  ↑↓ Scroll  c Call  x Extrinsic  e Export  s Submit finalized"
        } else {
            " Esc Back  ↑↓ Scroll  c Call  x Extrinsic  e Export  w Best/Finalized  s Submit"
        }
    } else {
        " Esc Back  ↑↓ Scroll  c Copy call"
    };
    f.render_widget(Paragraph::new(help).fg(Color::DarkGray), layout[2]);
}

fn draw_submit_confirmation(f: &mut Frame, app: &App) {
    let area = centered_rect(58, 7, f.area());
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(format!(
            "Submit the exact reviewed bytes and wait for {:?}?\n\nEnter/y Confirm   Esc/n Cancel",
            app.wait_for
        ))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirm submission")
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
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
    if matches!(app.focus, Focus::Form)
        && app.panel == Panel::Storage
        && let Some(ref form) = app.storage_form
    {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Parameters")
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(result_area);
        f.render_widget(block, result_area);
        form.render(f, inner);
        return;
    }
    if matches!(app.focus, Focus::BodyInput) && app.panel == Panel::Calls {
        let prompt = match app.call_input_mode {
            super::CallInputMode::Json => "Whole-body JSON",
            super::CallInputMode::ScaleText => "Whole-body SCALE text",
            super::CallInputMode::JsonFile => "JSON file path",
            super::CallInputMode::ScaleTextFile => "SCALE-text file path",
            super::CallInputMode::Typed => "Typed parameters",
        };
        f.render_widget(
            Paragraph::new(format!("{}█", app.call_body_input))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(prompt)
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
            result_area,
        );
        return;
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
    if matches!(app.focus, Focus::Form)
        && app.panel == Panel::Calls
        && let Some(ref form) = app.call_form
    {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Parameters")
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(result_area);
        f.render_widget(block, result_area);
        form.render(f, inner);
        return;
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

    let rev_idx = app.recent_blocks.len().saturating_sub(1 + detail.block_idx);
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
