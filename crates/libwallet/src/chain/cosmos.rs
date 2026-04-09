use crate::bip32::ExtendedKey;
use crate::chain::KeyStore;
use crate::vault::{utils::DerivedSigner, Vault};

/// Cosmos/Tendermint default BIP44 path (coin type 118)
const DEFAULT_PATH: &str = "m/44'/118'/0'/0/0";

/// A vault wrapper that produces Cosmos-compatible secp256k1 signers
/// using BIP32 hierarchical deterministic derivation.
///
/// ```ignore
/// let keys = Simple::from_phrase("...")?;
/// let mut vault = Cosmos::new(keys);
/// let signer = vault.unlock(None, ()).await?;
/// ```
pub struct Cosmos<K> {
    keys: K,
}

impl<K> Cosmos<K> {
    pub fn new(keys: K) -> Self {
        Cosmos { keys }
    }
}

impl<K: KeyStore> Vault for Cosmos<K> {
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
