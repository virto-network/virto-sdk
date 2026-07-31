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

/// Mutable lifecycle required by rotatable session secrets.
///
/// `upsert` replaces the single entry identified by the store instance; it
/// must not append another secure-store record. Implementations clear any
/// unlocked cache on replacement or deletion.
pub trait MutableKeyStore {
    type Error;

    fn upsert(&mut self, secret: &[u8]) -> Result<(), Self::Error>;
    fn delete(&mut self) -> Result<(), Self::Error>;
}

/// Deterministic secure-store entry id for one `(genesis, pass account)` pair.
pub fn pass_session_entry_id(
    genesis_hash: &[u8; 32],
    pass_account: &[u8; 32],
) -> alloc::string::String {
    let mut id = alloc::string::String::from("pass-session-v1-");
    push_hex(&mut id, genesis_hash);
    id.push('-');
    push_hex(&mut id, pass_account);
    id
}

fn push_hex(output: &mut alloc::string::String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

#[cfg(any(test, feature = "vault_os", feature = "vault_pass"))]
pub(crate) fn encode_raw_secret(secret: &[u8]) -> alloc::string::String {
    let mut encoded = alloc::string::String::from("libwallet-raw-v1:");
    push_hex(&mut encoded, secret);
    encoded
}

#[cfg(any(test, feature = "vault_os", feature = "vault_pass"))]
pub(crate) fn decode_raw_secret(value: &str) -> Option<alloc::vec::Vec<u8>> {
    let value = value.strip_prefix("libwallet-raw-v1:")?;
    if value.len() % 2 != 0 {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            Some((high << 4) | low)
        })
        .collect()
}

#[cfg(any(test, feature = "vault_os", feature = "vault_pass"))]
fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// Simple<S, N> is a KeyStore
impl<S, const N: usize> KeyStore for crate::vault::Simple<S, N> {
    type Error = crate::vault::simple::Error;
    fn unlock(&mut self) -> Result<&[u8], Self::Error> {
        self.unlock().map(|b| b.as_slice())
    }
}

impl<S, const N: usize> MutableKeyStore for crate::vault::Simple<S, N> {
    type Error = crate::vault::simple::Error;

    fn upsert(&mut self, secret: &[u8]) -> Result<(), Self::Error> {
        self.replace(secret)
    }

    fn delete(&mut self) -> Result<(), Self::Error> {
        self.clear();
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_entry_id_is_deterministic_and_pair_specific() {
        let a = pass_session_entry_id(&[1; 32], &[2; 32]);
        assert_eq!(a, pass_session_entry_id(&[1; 32], &[2; 32]));
        assert_ne!(a, pass_session_entry_id(&[1; 32], &[3; 32]));
        assert_ne!(a, pass_session_entry_id(&[4; 32], &[2; 32]));
    }

    #[test]
    fn raw_secret_encoding_roundtrips() {
        let encoded = encode_raw_secret(&[0, 1, 127, 255]);
        assert_eq!(
            decode_raw_secret(&encoded),
            Some(alloc::vec![0, 1, 127, 255])
        );
    }
}
