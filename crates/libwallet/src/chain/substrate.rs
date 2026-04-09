use crate::chain::KeyStore;
use crate::vault::{utils::{DerivedSigner, RootAccount}, Vault};

/// A vault wrapper that applies Substrate-compatible key derivation.
///
/// Wraps any entropy source (e.g. `Simple`) and produces sr25519 signers
/// derived using PBKDF2 + Substrate derivation paths.
///
/// ```ignore
/// let keys = Simple::from_phrase("...")?;
/// let mut vault = Substrate::new(keys);
/// let signer = vault.unlock(Some("//alice"), ()).await?;
/// ```
pub struct Substrate<K> {
    keys: K,
}

impl<K> Substrate<K> {
    pub fn new(keys: K) -> Self {
        Substrate { keys }
    }
}

impl<K: KeyStore> Vault for Substrate<K> {
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
        let root = RootAccount::from_bytes(&*seed).ok_or(VaultError::Derivation)?;
        let path = path.unwrap_or("//default");
        let pair = root.derive(path);
        Ok(DerivedSigner::new(pair, path))
    }
}
