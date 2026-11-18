//! Profile dialogs: a few text fields, edited the same way and submitted
//! together to the chain worker.

use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::*;
use zeroize::Zeroizing;

use super::chain::ToChain;
use super::{App, Focus};

pub struct Dialog {
    action: Action,
    title: &'static str,
    fields: Vec<Field>,
    focus: usize,
    notes: Vec<String>,
}

struct Field {
    label: &'static str,
    value: Zeroizing<String>,
    masked: bool,
    /// Values ←/→ cycle through; typing still edits the value.
    presets: Vec<String>,
}

impl Field {
    fn text(label: &'static str) -> Self {
        Self::with_presets(label, Vec::new())
    }

    fn masked(label: &'static str) -> Self {
        Self {
            masked: true,
            ..Self::text(label)
        }
    }

    fn with_presets(label: &'static str, presets: Vec<String>) -> Self {
        Self {
            label,
            value: Zeroizing::new(presets.first().cloned().unwrap_or_default()),
            masked: false,
            presets,
        }
    }

    fn cycle(&mut self, forward: bool) {
        let count = self.presets.len();
        if count == 0 {
            return;
        }
        let next = match self
            .presets
            .iter()
            .position(|preset| **preset == *self.value)
        {
            Some(index) if forward => (index + 1) % count,
            Some(index) => (index + count - 1) % count,
            None => 0,
        };
        *self.value = self.presets[next].clone();
    }
}

enum Action {
    ImportWallet,
    #[cfg(feature = "pass")]
    Session {
        profile: String,
    },
    #[cfg(feature = "pass")]
    ForgetSession {
        profile: String,
    },
    #[cfg(feature = "pass")]
    Enroll,
    #[cfg(feature = "pass")]
    AddDevice {
        profile: String,
    },
    /// `devices` match the device field's presets.
    #[cfg(feature = "pass")]
    RemoveDevice {
        profile: String,
        devices: Vec<[u8; 32]>,
    },
}

impl Dialog {
    pub fn import_wallet() -> Self {
        Self {
            action: Action::ImportWallet,
            title: "Import wallet profile",
            fields: vec![Field::text("Profile name"), Field::masked("Mnemonic")],
            focus: 0,
            notes: vec![
                "The mnemonic's root Sr25519 account is derived and verified before storage."
                    .into(),
            ],
        }
    }

    /// `selected` is the pallet and call selected in the explorer, offered as
    /// policy presets after the stored policy.
    #[cfg(feature = "pass")]
    pub fn session(
        profile: &crate::profiles::PassProfile,
        selected: Option<(String, String)>,
    ) -> Self {
        let mut presets: Vec<String> = profile
            .session
            .iter()
            .map(|session| session.policy.clone())
            .collect();
        if let Some((pallet, call)) = selected {
            presets.extend([
                format!("calls:{pallet}/{call}"),
                format!("pallets:{pallet}"),
            ]);
        }
        Self {
            action: Action::Session {
                profile: profile.name.clone(),
            },
            title: "Connect pass session",
            fields: vec![
                Field::with_presets(
                    "Policy (calls:pallet/call,... or pallets:pallet,...)",
                    presets,
                ),
                Field::text("Duration in blocks (empty for the runtime maximum)"),
            ],
            focus: 0,
            notes: vec![
                format!("Pass profile: {}", profile.name),
                "Exact on-chain sessions are reused; updates always open transaction review."
                    .into(),
            ],
        }
    }

    #[cfg(feature = "pass")]
    pub fn forget_session(profile: &str) -> Self {
        Self {
            action: Action::ForgetSession {
                profile: profile.into(),
            },
            title: "Forget local session",
            fields: Vec::new(),
            focus: 0,
            notes: vec![
                format!("Forget the local session for {profile}?"),
                pass::session::FORGET_SESSION_WARNING.into(),
            ],
        }
    }

    /// `devices` are the device kinds the runtime accepts, as presets.
    #[cfg(feature = "pass")]
    pub fn enroll(devices: Vec<String>) -> Self {
        Self {
            action: Action::Enroll,
            title: "Enroll pass account",
            fields: vec![
                Field::text("Pass profile name"),
                Field::text("Hashed user ID (exact 32-byte hex)"),
                Field::text("Registrar wallet profile"),
                Field::with_presets(DEVICE_LABEL, devices),
            ],
            focus: 0,
            notes: vec![
                "The predicted pass address and complete transaction are shown before submission."
                    .into(),
            ],
        }
    }

    #[cfg(feature = "pass")]
    pub fn add_device(profile: &str, devices: Vec<String>) -> Self {
        Self {
            action: Action::AddDevice {
                profile: profile.into(),
            },
            title: "Add pass device",
            fields: vec![
                Field::with_presets(DEVICE_LABEL, devices),
                Field::text("Filter (calls:..., pallets:..., or admin)"),
                Field::text("Admin confirmation (type ADMIN for an admin filter)"),
            ],
            focus: 0,
            notes: vec![
                format!("Pass profile: {profile}"),
                "Adding a device is authenticated directly by the profile's primary device.".into(),
            ],
        }
    }

    #[cfg(feature = "pass")]
    pub fn remove_device(profile: &crate::profiles::PassProfile) -> Self {
        let primary = std::iter::once(("primary", profile.device_id, &profile.device));
        let additional = profile
            .additional_devices
            .iter()
            .map(|record| ("additional", record.device_id, &record.device));
        let (labels, devices) = primary
            .chain(additional)
            .map(|(role, device_id, device)| {
                let label = format!(
                    "{role} · {} · 0x{}",
                    device_provider_label(device),
                    hex::encode(device_id)
                );
                (label, device_id)
            })
            .unzip();
        Self {
            action: Action::RemoveDevice {
                profile: profile.name.clone(),
                devices,
            },
            title: "Remove pass device",
            fields: vec![
                Field::with_presets("Device (←→ to select)", labels),
                Field::text("Type REMOVE"),
            ],
            focus: 0,
            notes: vec![
                format!("Pass profile: {}", profile.name),
                "Removing the primary device promotes the first additional one; removing the last \
                 known device leaves the profile unusable. Verify another usable and Admin device \
                 remains on-chain: local profiles do not know the on-chain filters."
                    .into(),
            ],
        }
    }

    fn value(&self, index: usize) -> &str {
        self.fields[index].value.trim()
    }

    /// The request to send and the status to show meanwhile.
    #[cfg_attr(not(feature = "pass"), allow(unused_variables))]
    fn request(&mut self, metadata: &sube::Metadata) -> Result<(ToChain, &'static str), String> {
        match &self.action {
            Action::ImportWallet => {
                if self.value(0).is_empty() || self.value(1).is_empty() {
                    return Err("Profile name and mnemonic are required.".into());
                }
                let name = self.value(0).to_owned();
                let mnemonic = std::mem::take(&mut self.fields[1].value);
                Ok((
                    ToChain::ImportWallet { name, mnemonic },
                    "Validating and importing wallet profile...",
                ))
            }
            #[cfg(feature = "pass")]
            Action::Session { profile } => {
                if self.value(0).is_empty() {
                    return Err("An explicit Calls or Pallets policy is required.".into());
                }
                let duration = match self.value(1) {
                    "" => None,
                    duration => Some(
                        duration
                            .parse::<u32>()
                            .map_err(|_| "Session duration must be a u32 block count.")?,
                    ),
                };
                Ok((
                    ToChain::PreparePassSession {
                        profile: profile.clone(),
                        policy: self.value(0).into(),
                        duration,
                    },
                    "Checking session and preparing registration...",
                ))
            }
            #[cfg(feature = "pass")]
            Action::ForgetSession { profile } => Ok((
                ToChain::ForgetPassSession(profile.clone()),
                "Deleting the local session secret...",
            )),
            #[cfg(feature = "pass")]
            Action::Enroll => {
                if (0..3).any(|field| self.value(field).is_empty()) {
                    return Err("Name, exact user ID and registrar are required.".into());
                }
                Ok((
                    ToChain::PrepareEnrollment(crate::workflow::EnrollmentRequest {
                        name: self.value(0).into(),
                        user_id: self.value(1).into(),
                        registrar: self.value(2).into(),
                        device: self.value(3).parse()?,
                    }),
                    "Creating device attestation and enrollment draft...",
                ))
            }
            #[cfg(feature = "pass")]
            Action::AddDevice { profile } => {
                let device = self.value(0).parse()?;
                let filter = pass::DeviceFilter::parse(metadata, self.value(1))
                    .map_err(|error| error.to_string())?;
                let admin = matches!(filter, pass::DeviceFilter::Admin);
                if admin && self.value(2) != "ADMIN" {
                    return Err("Type ADMIN in the separate confirmation field.".into());
                }
                Ok((
                    ToChain::PrepareDeviceAddition(crate::workflow::DeviceAdditionRequest {
                        profile: profile.clone(),
                        device,
                        filter: self.value(1).into(),
                        admin_confirmed: admin,
                    }),
                    "Creating the new device attestation...",
                ))
            }
            #[cfg(feature = "pass")]
            Action::RemoveDevice { profile, devices } => {
                let field = &self.fields[0];
                let selected = field
                    .presets
                    .iter()
                    .position(|label| **label == *field.value)
                    .ok_or("Select a known device with ←→.")?;
                if self.value(1) != "REMOVE" {
                    return Err("Type REMOVE to prepare this device removal.".into());
                }
                Ok((
                    ToChain::PrepareDeviceRemoval(crate::workflow::DeviceRemovalRequest {
                        profile: profile.clone(),
                        device_id: devices[selected],
                    }),
                    "Authenticating and preparing device removal...",
                ))
            }
        }
    }
}

#[cfg(feature = "pass")]
const DEVICE_LABEL: &str = "Device (wallet:PROFILE or webauthn:RP_ID@ORIGIN)";

/// The device kinds this build supports and the runtime accepts, as
/// `--device` prefixes.
#[cfg(feature = "pass")]
pub fn device_presets(metadata: &sube::Metadata) -> Vec<String> {
    let Ok(config) = pass::PassRuntimeConfig::discover(metadata) else {
        return Vec::new();
    };
    let kinds: &[(&str, &str)] = &[
        ("SubstrateKey", "wallet:"),
        #[cfg(feature = "desktop-webauthn")]
        ("WebAuthn", "webauthn:"),
    ];
    kinds
        .iter()
        .filter(|(variant, _)| config.supports_attestation(variant))
        .map(|(_, prefix)| prefix.to_string())
        .collect()
}

#[cfg(feature = "pass")]
fn device_provider_label(device: &crate::profiles::DeviceProviderProfile) -> &'static str {
    match device {
        crate::profiles::DeviceProviderProfile::SubstrateKey { .. } => "Substrate key",
        crate::profiles::DeviceProviderProfile::WebAuthn { .. } => "WebAuthn",
    }
}

pub fn handle_key(app: &mut App, key: KeyCode) {
    let Some(dialog) = app.dialog.as_mut() else {
        app.focus = Focus::Profiles;
        return;
    };
    let last = dialog.fields.len().saturating_sub(1);
    let field = dialog.fields.get_mut(dialog.focus);
    match key {
        KeyCode::Esc => {
            app.dialog = None;
            app.error = None;
            app.focus = Focus::Profiles;
        }
        KeyCode::Tab | KeyCode::Down => dialog.focus = (dialog.focus + 1).min(last),
        KeyCode::BackTab | KeyCode::Up => dialog.focus = dialog.focus.saturating_sub(1),
        KeyCode::Left | KeyCode::Right => {
            if let Some(field) = field {
                field.cycle(key == KeyCode::Right);
            }
        }
        KeyCode::Backspace => {
            if let Some(field) = field {
                field.value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(field) = field {
                field.value.push(c);
            }
        }
        KeyCode::Enter if dialog.focus < last => dialog.focus += 1,
        KeyCode::Enter => match dialog.request(&app.meta) {
            Ok((request, status)) => {
                app.review_returns_to_profiles = true;
                app.error = Some(status.into());
                let _ = app.to_chain.send(request);
            }
            Err(error) => app.error = Some(error),
        },
        _ => {}
    }
}

pub fn draw(f: &mut Frame, dialog: &Dialog, error: Option<&str>) {
    let highlighted = Style::default().fg(Color::Yellow).bold();
    let mut lines: Vec<Line> = dialog
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let value = if field.masked {
                "•".repeat(field.value.chars().count())
            } else {
                field.value.to_string()
            };
            let style = if index == dialog.focus {
                highlighted
            } else {
                Style::default()
            };
            Line::styled(format!("{}: {value}", field.label), style)
        })
        .collect();
    lines.push(Line::raw(""));
    lines.extend(dialog.notes.iter().map(|note| Line::raw(note.as_str())));
    lines.push(Line::raw(
        "Tab fields  ←→ presets  Enter continue  Esc cancel",
    ));
    lines.push(Line::styled(
        error.unwrap_or(""),
        Style::default().fg(Color::Red),
    ));
    let height = lines.len() as u16 + 2 + dialog.notes.len() as u16;
    let area = super::draw::centered_rect(100, height, f.area());
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(dialog.title)),
        area,
    );
}

#[cfg(all(test, feature = "pass"))]
mod tests {
    use super::*;

    fn kreivo() -> sube::Metadata {
        sube::Metadata::from_bytes(include_bytes!("../../../tests/fixtures/kreivo.scale")).unwrap()
    }

    fn type_into(dialog: &mut Dialog, field: usize, text: &str) {
        *dialog.fields[field].value = text.into();
    }

    #[test]
    fn presets_cycle_both_ways_and_typing_edits_them() {
        let mut field = Field::with_presets("Device", vec!["wallet:".into(), "webauthn:".into()]);
        assert_eq!(*field.value, "wallet:");
        field.cycle(true);
        assert_eq!(*field.value, "webauthn:");
        field.cycle(true);
        assert_eq!(*field.value, "wallet:");
        field.cycle(false);
        assert_eq!(*field.value, "webauthn:");
        field.value.push_str("example.com@https://example.com");
        field.cycle(true);
        assert_eq!(*field.value, "wallet:");
    }

    #[test]
    fn admin_device_filters_need_the_typed_confirmation() {
        let metadata = kreivo();
        let mut dialog = Dialog::add_device("pass", vec!["wallet:".into()]);
        type_into(&mut dialog, 0, "wallet:device");
        type_into(&mut dialog, 1, "admin");
        let error = dialog.request(&metadata).err().unwrap();
        assert!(error.contains("Type ADMIN"), "{error}");

        type_into(&mut dialog, 2, "ADMIN");
        let Ok((ToChain::PrepareDeviceAddition(request), _)) = dialog.request(&metadata) else {
            panic!("expected a device addition");
        };
        assert!(request.admin_confirmed);
        assert_eq!(
            request.device,
            crate::profiles::DeviceProviderProfile::SubstrateKey {
                wallet: "device".into()
            }
        );
    }

    #[test]
    fn device_removal_sends_the_selected_device_once_confirmed() {
        let metadata = kreivo();
        let device = crate::profiles::DeviceProviderProfile::SubstrateKey {
            wallet: "device".into(),
        };
        let profile = crate::profiles::PassProfile {
            name: "pass".into(),
            genesis_hash: [1; 32],
            pass_account: [2; 32],
            user_id: [3; 32],
            device_id: [4; 32],
            device: device.clone(),
            additional_devices: vec![crate::profiles::DeviceRecord {
                device_id: [5; 32],
                device,
            }],
            session: None,
        };
        let mut dialog = Dialog::remove_device(&profile);
        dialog.fields[0].cycle(true);
        assert!(dialog.request(&metadata).is_err(), "REMOVE was not typed");
        type_into(&mut dialog, 1, "REMOVE");
        let Ok((ToChain::PrepareDeviceRemoval(request), _)) = dialog.request(&metadata) else {
            panic!("expected a device removal");
        };
        assert_eq!(request.device_id, [5; 32]);
    }
}
