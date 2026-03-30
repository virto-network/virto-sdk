use crate::bip32::ExtendedKey;
use crate::vault::{utils::DerivedSigner, Vault};

const DEFAULT_PATH: &str = "m/44'/60'/0'/0/0";

/// A vault wrapper that produces Ethereum-compatible secp256k1 signers
/// using BIP32 hierarchical deterministic derivation.
///
/// ```ignore
/// let keys = Simple::from_phrase("...");
/// let mut vault = Ethereum::new(keys);
/// // Default path: m/44'/60'/0'/0/0
/// let signer = vault.unlock(None, ()).await?;
/// // Custom path:
/// let signer = vault.unlock(Some("m/44'/60'/0'/0/1"), ()).await?;
/// ```
pub struct Ethereum<K> {
    keys: K,
}

impl<K> Ethereum<K> {
    pub fn new(keys: K) -> Self {
        Ethereum { keys }
    }
}

/// Derive an Ethereum address (last 20 bytes of keccak256(uncompressed_pubkey))
pub fn eth_address(pubkey: &k256::ecdsa::VerifyingKey) -> [u8; 20] {
    use sha3::{Keccak256, Digest};
    let uncompressed = pubkey.to_encoded_point(false);
    let mut hasher = Keccak256::new();
    hasher.update(&uncompressed.as_bytes()[1..]); // skip 0x04 prefix
    let hash = hasher.finalize();
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..32]);
    addr
}

impl<K: crate::substrate_ext::KeyStore> Vault for Ethereum<K> {
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
        let derived = ExtendedKey::from_seed(&*seed)
            .and_then(|master| master.derive_path(path))
            .ok_or(VaultError::Derivation)?;

        let pair = <crate::key_pair::secp256k1::Pair as crate::Pair>::from_bytes(derived.secret_key())
            .ok_or(VaultError::Derivation)?;

        Ok(DerivedSigner::new(pair.into(), path))
    }
}
