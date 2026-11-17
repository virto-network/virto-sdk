use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const PROFILE_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Profile {
    Wallet(WalletProfile),
    #[cfg(feature = "pass")]
    Pass(PassProfile),
}

impl Profile {
    pub fn name(&self) -> &str {
        match self {
            Self::Wallet(profile) => &profile.name,
            #[cfg(feature = "pass")]
            Self::Pass(profile) => &profile.name,
        }
    }

    pub fn genesis_hash(&self) -> [u8; 32] {
        match self {
            Self::Wallet(profile) => profile.genesis_hash,
            #[cfg(feature = "pass")]
            Self::Pass(profile) => profile.genesis_hash,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalletProfile {
    pub name: String,
    pub genesis_hash: [u8; 32],
    /// The sr25519 account derived at `//default`.
    pub account: [u8; 32],
}

#[cfg(feature = "wallet")]
impl WalletProfile {
    /// The OS secure-store entry holding the mnemonic.
    pub fn secure_entry(&self) -> String {
        format!("wallet-profile-v1-{}", self.name)
    }
}

#[cfg(feature = "pass")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PassProfile {
    pub name: String,
    pub genesis_hash: [u8; 32],
    pub pass_account: [u8; 32],
    pub user_id: [u8; 32],
    pub device_id: [u8; 32],
    pub device: DeviceProviderProfile,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_devices: Vec<DeviceRecord>,
    pub session: Option<SessionProfile>,
}

#[cfg(feature = "pass")]
impl PassProfile {
    /// The OS secure-store entry holding the session key.
    pub fn session_entry(&self) -> String {
        libwallet::pass_session_entry_id(&self.genesis_hash, &self.pass_account)
    }

    /// Forget `device_id`. Removing the primary device promotes the first
    /// additional one, if any.
    pub fn remove_device(&mut self, device_id: [u8; 32]) {
        self.additional_devices
            .retain(|device| device.device_id != device_id);
        if self.device_id == device_id && !self.additional_devices.is_empty() {
            let promoted = self.additional_devices.remove(0);
            self.device_id = promoted.device_id;
            self.device = promoted.device;
        }
    }
}

#[cfg(feature = "pass")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "provider", rename_all = "kebab-case")]
pub enum DeviceProviderProfile {
    SubstrateKey {
        wallet: String,
    },
    WebAuthn {
        rp_id: String,
        origin: String,
        credential_id: Vec<u8>,
    },
    SshAgent {
        fingerprint: String,
        namespace: String,
    },
}

#[cfg(feature = "pass")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceRecord {
    pub device_id: [u8; 32],
    pub device: DeviceProviderProfile,
}

#[cfg(feature = "pass")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionProfile {
    pub account: [u8; 32],
    pub policy: String,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profiles {
    pub version: u32,
    pub active: Option<String>,
    pub profiles: Vec<Profile>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            version: PROFILE_VERSION,
            active: None,
            profiles: Vec::new(),
        }
    }
}

impl Profiles {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                let profiles: Self =
                    serde_json::from_slice(&bytes).context("decode profile store")?;
                if profiles.version != PROFILE_VERSION {
                    anyhow::bail!("unsupported profile store version {}", profiles.version);
                }
                Ok(profiles)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).context("read profile store"),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("create profile directory")?;
        }
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).context("encode profile store")?;
        fs::write(&temporary, bytes).context("write temporary profile store")?;
        fs::rename(&temporary, path).context("atomically replace profile store")
    }

    pub fn find(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.name() == name)
    }

    #[cfg(feature = "wallet")]
    pub fn active(&self) -> Option<&Profile> {
        self.active.as_deref().and_then(|name| self.find(name))
    }

    #[cfg(feature = "pass")]
    pub fn pass_mut(&mut self, name: &str) -> Result<&mut PassProfile> {
        match self
            .profiles
            .iter_mut()
            .find(|profile| profile.name() == name)
        {
            Some(Profile::Pass(profile)) => Ok(profile),
            _ => anyhow::bail!("pass profile {name:?} not found"),
        }
    }

    pub fn upsert(&mut self, profile: Profile) {
        if let Some(existing) = self
            .profiles
            .iter_mut()
            .find(|existing| existing.name() == profile.name())
        {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
    }

    pub fn activate(&mut self, name: &str, genesis_hash: [u8; 32]) -> Result<()> {
        let profile = self
            .find(name)
            .with_context(|| format!("profile {name:?} not found"))?;
        if profile.genesis_hash() != genesis_hash {
            anyhow::bail!("profile genesis hash does not match the connected chain");
        }
        self.active = Some(name.into());
        Ok(())
    }
}

pub fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("sube").join("profiles.json");
    }
    if cfg!(windows)
        && let Some(path) = std::env::var_os("APPDATA")
    {
        return PathBuf::from(path).join("sube").join("profiles.json");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("sube")
        .join("profiles.json")
}

#[cfg(feature = "wallet")]
pub fn parse_hash(value: &str, what: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(value.trim_start_matches("0x"))
        .with_context(|| format!("{what} must be 32-byte hex"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{what} must be exactly 32 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_rejects_a_genesis_mismatch() {
        let mut profiles = Profiles::default();
        profiles.upsert(Profile::Wallet(WalletProfile {
            name: "alice".into(),
            genesis_hash: [1; 32],
            account: [2; 32],
        }));
        assert!(profiles.activate("alice", [3; 32]).is_err());
        assert_eq!(profiles.active, None);
    }

    #[test]
    fn upsert_keeps_a_constant_profile_count() {
        let mut profiles = Profiles::default();
        for account in [[2; 32], [3; 32]] {
            profiles.upsert(Profile::Wallet(WalletProfile {
                name: "alice".into(),
                genesis_hash: [1; 32],
                account,
            }));
        }
        assert_eq!(profiles.profiles.len(), 1);
    }

    /// Drafts and derivable fields used to be persisted; such a store still
    /// loads and is saved without them.
    #[test]
    fn stores_with_dropped_fields_still_load() {
        let stored = r#"{
            "version": 1,
            "active": "alice",
            "profiles": [{
                "type": "wallet",
                "name": "alice",
                "genesis_hash": [1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],
                "account": [2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2],
                "secure_entry": "wallet-profile-v1-alice",
                "scheme": "sr25519"
            }],
            "pending_enrollments": [{"name": "pass", "valid_through": 12}],
            "pending_devices": [{"profile": "pass", "valid_through": 12}]
        }"#;
        let profiles: Profiles = serde_json::from_str(stored).unwrap();
        assert_eq!(profiles.find("alice").unwrap().genesis_hash(), [1; 32]);
        let saved = serde_json::to_string(&profiles).unwrap();
        for dropped in ["pending", "secure_entry", "scheme"] {
            assert!(!saved.contains(dropped), "{dropped}");
        }
    }

    #[test]
    #[cfg(feature = "pass")]
    fn pass_sessions_with_a_stored_entry_name_still_load() {
        let stored = r#"{
            "type": "pass",
            "name": "pass",
            "genesis_hash": [1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],
            "pass_account": [2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2],
            "user_id": [3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3],
            "device_id": [4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4],
            "device": {"provider": "substrate-key", "wallet": "alice"},
            "session": {
                "account": [5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5],
                "secure_entry": "pass-session-v1-old",
                "policy": "calls:System/remark",
                "expires_at": 99
            }
        }"#;
        let Profile::Pass(profile) = serde_json::from_str(stored).unwrap() else {
            panic!("expected a pass profile");
        };
        assert_eq!(profile.session.unwrap().expires_at, 99);
    }

    #[test]
    #[cfg(feature = "pass")]
    fn webauthn_profiles_store_only_public_identifiers() {
        let profile = PassProfile {
            name: "passkey".into(),
            genesis_hash: [1; 32],
            pass_account: [2; 32],
            user_id: [3; 32],
            device_id: [4; 32],
            device: DeviceProviderProfile::WebAuthn {
                rp_id: "example.com".into(),
                origin: "https://example.com".into(),
                credential_id: vec![5, 6, 7],
            },
            additional_devices: Vec::new(),
            session: None,
        };
        let encoded = serde_json::to_string(&profile).unwrap();
        assert!(encoded.contains("credential_id"));
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("secret"));
    }

    #[test]
    #[cfg(feature = "pass")]
    fn ssh_agent_profiles_store_only_selection_metadata() {
        let device = DeviceProviderProfile::SshAgent {
            fingerprint: "SHA256:example".into(),
            namespace: "virto-pass".into(),
        };
        let encoded = serde_json::to_string(&device).unwrap();
        assert!(encoded.contains("SHA256:example"));
        assert!(encoded.contains("virto-pass"));
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("secret"));
    }
}
