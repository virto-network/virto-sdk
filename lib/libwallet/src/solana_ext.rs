use crate::bip32::Slip10Key;
use crate::vault::{utils::DerivedSigner, Vault};

/// Solana default BIP44 path (all hardened per SLIP-0010)
const DEFAULT_PATH: &str = "m/44'/501'/0'/0'";

/// A vault wrapper that produces Solana-compatible ed25519 signers
/// using SLIP-0010 hierarchical deterministic derivation.
///
/// ```ignore
/// let keys = Simple::from_phrase("...");
/// let mut vault = Solana::new(keys);
/// let signer = vault.unlock(None, ()).await?;
/// // Custom account index:
/// let signer = vault.unlock(Some("m/44'/501'/1'/0'"), ()).await?;
/// ```
pub struct Solana<K> {
    keys: K,
}

impl<K> Solana<K> {
    pub fn new(keys: K) -> Self {
        Solana { keys }
    }
}

impl<K: crate::substrate_ext::KeyStore> Vault for Solana<K> {
    type Credentials = ();
    type Error = crate::vault::VaultError<K::Error>;
    type Id = Option<&'static str>;
    type Signer = DerivedSigner;

    async fn unlock(
        &mut self,
        path: Self::Id,
        _creds: impl Into<Self::Credentials>,
    ) -> Result<Self::Signer, Self::Error> {
        use crate::vault::VaultError;
        let entropy = self.keys.unlock().map_err(VaultError::KeyStore)?;
        let seed = crate::substrate_ext::substrate_seed(entropy, "");

        let path = path.unwrap_or(DEFAULT_PATH);
        let derived = Slip10Key::from_seed(&*seed)
            .and_then(|master| master.derive_path(path))
            .ok_or(VaultError::Derivation)?;

        let pair = <crate::key_pair::ed25519::Pair as crate::Pair>::from_bytes(derived.secret_key())
            .ok_or(VaultError::Derivation)?;

        Ok(DerivedSigner::new(pair.into(), path))
    }
}
