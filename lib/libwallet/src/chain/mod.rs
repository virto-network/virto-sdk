//! Chain-specific vault wrappers.
//!
//! Each submodule wraps a [`KeyStore`] to produce signers compatible
//! with a particular blockchain, handling derivation paths and key types.

#[cfg(feature = "substrate")]
pub mod substrate;
#[cfg(feature = "substrate")]
pub use substrate::Substrate;

#[cfg(feature = "ethereum")]
pub mod ethereum;
#[cfg(feature = "ethereum")]
pub use ethereum::Ethereum;

#[cfg(feature = "bitcoin")]
pub mod bitcoin;
#[cfg(feature = "bitcoin")]
pub use bitcoin::Bitcoin;

#[cfg(feature = "solana")]
pub mod solana;
#[cfg(feature = "solana")]
pub use solana::Solana;

#[cfg(feature = "cosmos")]
pub mod cosmos;
#[cfg(feature = "cosmos")]
pub use cosmos::Cosmos;

/// Trait for types that can provide raw entropy for key derivation.
///
/// Implemented by vault backends (`Simple`, `Pass`, `OSKeyring`).
/// Chain-specific wrappers like `Substrate<K>` and `Ethereum<K>`
/// require `K: KeyStore`.
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

/// Derive a 64-byte seed from mnemonic entropy using BIP39-compatible
/// PBKDF2-HMAC-SHA512 key derivation (2048 rounds, salt = "mnemonic" + passphrase).
///
/// This is the standard BIP39 seed derivation shared by Substrate, Ethereum,
/// Bitcoin, Solana, and Cosmos wallets.
#[cfg(all(feature = "pbkdf2", feature = "hmac", feature = "sha2"))]
pub fn seed_from_entropy(entropy: &[u8], passphrase: &str) -> zeroize::Zeroizing<[u8; 64]> {
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

impl From<&str> for crate::Network {
    fn from(s: &str) -> Self {
        use crate::Network;
        match s {
            "polkadot" => Network::Substrate(0),
            "kusama" => Network::Substrate(2),
            "kreivo" => Network::Substrate(2),
            "ethereum" => Network::Ethereum(1),
            "polygon" => Network::Ethereum(137),
            "bitcoin" => Network::Bitcoin,
            _ => Network::Substrate(42),
        }
    }
}
