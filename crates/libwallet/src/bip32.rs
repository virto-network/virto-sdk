//! Minimal BIP32/SLIP-0010 hierarchical deterministic key derivation.
//! BIP32 (secp256k1) uses HMAC-SHA512 + k256 scalar arithmetic.
//! SLIP-0010 (ed25519) uses HMAC-SHA512 with hardened-only derivation.

use hmac::{Hmac, Mac};
use sha2::Sha512;
use zeroize::Zeroize;

type HmacSha512 = Hmac<Sha512>;

/// A BIP32 extended private key (key + chain code) for secp256k1.
#[cfg(feature = "secp256k1")]
pub struct ExtendedKey {
    key: [u8; 32],
    chain_code: [u8; 32],
}

#[cfg(feature = "secp256k1")]
impl ExtendedKey {
    /// Derive master key from BIP39 seed using HMAC-SHA512("Bitcoin seed", seed).
    pub fn from_seed(seed: &[u8]) -> Option<Self> {
        let mut mac = HmacSha512::new_from_slice(b"Bitcoin seed").ok()?;
        mac.update(seed);
        let mut result = mac.finalize().into_bytes();

        let mut key = [0u8; 32];
        let mut chain_code = [0u8; 32];
        key.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..64]);
        result.zeroize();

        // Validate key is valid (non-zero, < curve order)
        if k256::ecdsa::SigningKey::from_slice(&key).is_err() {
            key.zeroize();
            chain_code.zeroize();
            return None;
        }

        Some(ExtendedKey { key, chain_code })
    }

    /// Derive a child key at the given index.
    /// Hardened indices have bit 31 set (>= 0x80000000).
    pub fn derive_child(&self, index: u32) -> Option<Self> {
        let is_hardened = index >= 0x80000000;

        let mut mac = HmacSha512::new_from_slice(&self.chain_code).ok()?;

        if is_hardened {
            mac.update(&[0x00]);
            mac.update(&self.key);
        } else {
            let signing_key = k256::ecdsa::SigningKey::from_slice(&self.key).ok()?;
            let pubkey = signing_key.verifying_key().to_encoded_point(true);
            mac.update(pubkey.as_bytes());
        }
        mac.update(&index.to_be_bytes());

        let mut result = mac.finalize().into_bytes();

        let mut il = [0u8; 32];
        let mut chain_code = [0u8; 32];
        il.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..64]);
        result.zeroize();

        use k256::elliptic_curve::ops::Reduce;
        let tweak = <k256::Scalar as Reduce<k256::U256>>::reduce_bytes(&il.into());
        let parent = <k256::Scalar as Reduce<k256::U256>>::reduce_bytes(&self.key.into());
        il.zeroize();
        let child = tweak + parent;

        if child.is_zero().into() {
            chain_code.zeroize();
            return None;
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&child.to_bytes());

        Some(ExtendedKey { key, chain_code })
    }

    /// Derive a key following a BIP32 path (e.g. m/44'/60'/0'/0/0).
    pub fn derive_path(mut self, path: &str) -> Option<Self> {
        for segment in path.split('/') {
            match segment {
                "m" | "" => continue,
                s => {
                    let (num, hardened) = if let Some(n) = s.strip_suffix('\'') {
                        (n, true)
                    } else {
                        (s, false)
                    };
                    let index: u32 = num.parse().ok()?;
                    let index = if hardened { index | 0x80000000 } else { index };
                    self = self.derive_child(index)?;
                }
            }
        }
        Some(self)
    }

    /// Get the raw 32-byte private key.
    pub fn secret_key(&self) -> &[u8; 32] {
        &self.key
    }
}

/// SLIP-0010 master key derivation for ed25519.
/// Uses "ed25519 seed" as the HMAC key instead of "Bitcoin seed".
/// All child derivations must be hardened for ed25519.
pub struct Slip10Key {
    key: [u8; 32],
    chain_code: [u8; 32],
}

impl Slip10Key {
    pub fn from_seed(seed: &[u8]) -> Option<Self> {
        let mut mac = HmacSha512::new_from_slice(b"ed25519 seed").ok()?;
        mac.update(seed);
        let mut result = mac.finalize().into_bytes();

        let mut key = [0u8; 32];
        let mut chain_code = [0u8; 32];
        key.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..64]);
        result.zeroize();

        Some(Slip10Key { key, chain_code })
    }

    /// Derive a hardened child. For ed25519, only hardened derivation is valid.
    pub fn derive_child(&self, index: u32) -> Option<Self> {
        let index = index | 0x80000000; // force hardened

        let mut mac = HmacSha512::new_from_slice(&self.chain_code).ok()?;
        mac.update(&[0x00]);
        mac.update(&self.key);
        mac.update(&index.to_be_bytes());

        let mut result = mac.finalize().into_bytes();

        let mut key = [0u8; 32];
        let mut chain_code = [0u8; 32];
        key.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..64]);
        result.zeroize();

        Some(Slip10Key { key, chain_code })
    }

    /// Derive following a BIP32 path. All indices are forced hardened.
    pub fn derive_path(mut self, path: &str) -> Option<Self> {
        for segment in path.split('/') {
            match segment {
                "m" | "" => continue,
                s => {
                    let num = s.strip_suffix('\'').unwrap_or(s);
                    let index: u32 = num.parse().ok()?;
                    self = self.derive_child(index)?;
                }
            }
        }
        Some(self)
    }

    pub fn secret_key(&self) -> &[u8; 32] {
        &self.key
    }
}

impl Drop for Slip10Key {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.key.zeroize();
        self.chain_code.zeroize();
    }
}

#[cfg(feature = "secp256k1")]
impl Drop for ExtendedKey {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.key.zeroize();
        self.chain_code.zeroize();
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use alloc::{format, string::String, vec::Vec};

    #[test]
    #[cfg(feature = "secp256k1")]
    fn bip32_test_vector_1() {
        // BIP32 test vector 1
        // Seed: 000102030405060708090a0b0c0d0e0f
        let seed = hex_to_bytes("000102030405060708090a0b0c0d0e0f");
        let master = ExtendedKey::from_seed(&seed).unwrap();

        // Master key (from BIP32 spec)
        assert_eq!(
            bytes_to_hex(master.secret_key()),
            "e8f32e723decf4051aefac8e2c93c9c5b214313817cdb01a1494b917c8436b35"
        );

        // m/0' derivation
        let child = master.derive_child(0x80000000).unwrap();
        assert_eq!(
            bytes_to_hex(child.secret_key()),
            "edb2e14f9ee77d26dd93b4ecede8d16ed408ce149b6cd80b0715a2d911a0afea"
        );
    }

    #[test]
    #[cfg(feature = "secp256k1")]
    fn derive_ethereum_default_path() {
        // Seed from "abandon" x11 + "about" mnemonic (well-known test vector)
        let seed = hex_to_bytes(
            "5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4"
        );
        let key = ExtendedKey::from_seed(&seed)
            .unwrap()
            .derive_path("m/44'/60'/0'/0/0")
            .unwrap();

        // Known Ethereum private key for this mnemonic at m/44'/60'/0'/0/0
        assert_eq!(
            bytes_to_hex(key.secret_key()),
            "1ab42cc412b618bdea3a599e3c9bae199ebf030895b039e9db1e30dafb12b727"
        );
    }

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    fn bytes_to_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
