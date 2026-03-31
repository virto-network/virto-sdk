use crate::bip32::ExtendedKey;
use crate::chain::KeyStore;
use crate::vault::{utils::DerivedSigner, Vault};

/// BIP44 path for Bitcoin
const DEFAULT_PATH: &str = "m/44'/0'/0'/0/0";
/// BIP84 path for native SegWit (bech32)
pub const SEGWIT_PATH: &str = "m/84'/0'/0'/0/0";

/// A vault wrapper that produces Bitcoin-compatible secp256k1 signers
/// using BIP32 hierarchical deterministic derivation.
///
/// ```ignore
/// let keys = Simple::from_phrase("...")?;
/// let mut vault = Bitcoin::new(keys);
/// // Default BIP44 path: m/44'/0'/0'/0/0
/// let signer = vault.unlock(None, ()).await?;
/// // SegWit path:
/// let signer = vault.unlock(Some("m/84'/0'/0'/0/0"), ()).await?;
/// ```
pub struct Bitcoin<K> {
    keys: K,
}

impl<K> Bitcoin<K> {
    pub fn new(keys: K) -> Self {
        Bitcoin { keys }
    }
}

impl<K: KeyStore> Vault for Bitcoin<K> {
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
        let seed = crate::chain::seed_from_entropy(entropy, "");

        let path = path.unwrap_or(DEFAULT_PATH);
        let derived = ExtendedKey::from_seed(&*seed)
            .and_then(|master| master.derive_path(path))
            .ok_or(VaultError::Derivation)?;

        let pair = <crate::key_pair::secp256k1::Pair as crate::Pair>::from_bytes(derived.secret_key())
            .ok_or(VaultError::Derivation)?;

        Ok(DerivedSigner::new(pair.into(), path))
    }
}
