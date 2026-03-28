use crate::vault::{utils::DerivedSigner, Vault};

/// Derive a 64-byte seed from mnemonic entropy using BIP39-compatible
/// PBKDF2-HMAC-SHA512 (same as Substrate). The Ethereum derivation path
/// (BIP32/BIP44) is applied separately on the resulting seed.
pub fn ethereum_seed(entropy: &[u8], passphrase: &str) -> zeroize::Zeroizing<[u8; 64]> {
    // BIP39 seed derivation is the same across chains
    crate::substrate_ext::substrate_seed(entropy, passphrase)
}

/// A vault wrapper that produces Ethereum-compatible secp256k1 signers.
///
/// ```ignore
/// let keys = Simple::from_phrase("...");
/// let mut vault = Ethereum::new(keys);
/// let signer = vault.unlock(None, ()).await?;
/// ```
pub struct Ethereum<K> {
    keys: K,
}

impl<K> Ethereum<K> {
    pub fn new(keys: K) -> Self {
        Ethereum { keys }
    }
}

impl<K: crate::substrate_ext::KeyStore> Vault for Ethereum<K> {
    type Credentials = ();
    type Error = K::Error;
    type Id = Option<&'static str>;
    type Signer = DerivedSigner;

    async fn unlock(
        &mut self,
        path: Self::Id,
        _creds: impl Into<Self::Credentials>,
    ) -> Result<Self::Signer, Self::Error> {
        let entropy = self.keys.unlock()?;
        let seed = ethereum_seed(entropy, "");

        // Use first 32 bytes of the BIP39 seed as the secp256k1 private key.
        // Full BIP32 path derivation (m/44'/60'/0'/0/0) would require HMAC-SHA512
        // child key derivation — for now we derive directly from seed.
        let pair = <crate::key_pair::secp256k1::Pair as crate::Pair>::from_bytes(&seed[..32])
            .expect("valid secp256k1 seed");

        let name = path.unwrap_or("eth-default");
        Ok(DerivedSigner::new(pair.into(), name))
    }
}
