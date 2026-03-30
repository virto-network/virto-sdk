use crate::Network;
use crate::vault::{utils::{DerivedSigner, RootAccount}, Vault};

impl From<&str> for Network {
    fn from(s: &str) -> Self {
        match s {
            "polkadot" => Network::Substrate(0),
            "kusama" => Network::Substrate(2),
            "karura" => Network::Substrate(8),
            "ethereum" => Network::Ethereum(1),
            "polygon" => Network::Ethereum(137),
            "bitcoin" => Network::Bitcoin,
            _ => Network::Substrate(42),
        }
    }
}

/// Derive a 64-byte seed from mnemonic entropy using the Substrate-compatible
/// PBKDF2-HMAC-SHA512 key derivation. Compatible with polkadot-js and subkey.
///
/// When `passphrase` is empty the salt is just `"mnemonic"` (BIP39 standard),
/// producing the same addresses as other Substrate wallets.
pub fn substrate_seed(entropy: &[u8], passphrase: &str) -> zeroize::Zeroizing<[u8; 64]> {
    use hmac::Hmac;
    use pbkdf2::pbkdf2;
    use sha2::Sha512;

    let mut seed = zeroize::Zeroizing::new([0u8; 64]);

    if passphrase.is_empty() {
        pbkdf2::<Hmac<Sha512>>(entropy, b"mnemonic", 2048, seed.as_mut());
    } else {
        let pass_bytes = passphrase.as_bytes();
        let salt_len = 8 + pass_bytes.len();
        let mut salt = [0u8; 8 + 256];
        salt[..8].copy_from_slice(b"mnemonic");
        let copy_len = pass_bytes.len().min(256);
        salt[8..8 + copy_len].copy_from_slice(&pass_bytes[..copy_len]);
        pbkdf2::<Hmac<Sha512>>(entropy, &salt[..salt_len.min(8 + 256)], 2048, seed.as_mut());
    }

    seed
}

/// A vault wrapper that applies Substrate-compatible key derivation.
///
/// Wraps any entropy source (e.g. `Simple`) and produces sr25519 signers
/// derived using PBKDF2 + Substrate derivation paths.
///
/// ```ignore
/// let keys = Simple::from_phrase("...");
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

/// Trait for types that can provide raw entropy for key derivation.
pub trait KeyStore {
    type Error;
    fn unlock(&mut self) -> Result<&[u8], Self::Error>;
}

// Simple<S, N> is a KeyStore
impl<S, const N: usize> KeyStore for crate::vault::Simple<S, N> {
    type Error = crate::vault::simple::Error;
    fn unlock(&mut self) -> Result<&[u8], Self::Error> {
        self.unlock().map(|b| b.as_slice())
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
        let seed = substrate_seed(entropy, "");
        let root = RootAccount::from_bytes(&*seed).ok_or(VaultError::Derivation)?;
        let path = path.unwrap_or("//default");
        let pair = root.derive(path);
        Ok(DerivedSigner::new(pair, path))
    }
}
