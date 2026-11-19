//! Hashing and hex helpers.

use blake2::{Blake2b, Digest, digest::typenum::U32};

use crate::Hash;

pub fn twox128(b: &[u8]) -> [u8; 16] {
    let mut o = [0u8; 16];
    o[..8].copy_from_slice(&twox64(b));
    o[8..].copy_from_slice(&twox_hash::XxHash64::oneshot(1, b).to_le_bytes());
    o
}

pub fn twox64(b: &[u8]) -> [u8; 8] {
    twox_hash::XxHash64::oneshot(0, b).to_le_bytes()
}

pub fn blake2_256(b: &[u8]) -> Hash {
    Blake2b::<U32>::digest(b).into()
}

/// `twox128(pallet) ++ twox128(item)`, the key of a FRAME storage value.
pub fn storage_key(pallet: &str, item: &str) -> Vec<u8> {
    [twox128(pallet.as_bytes()), twox128(item.as_bytes())].concat()
}

pub fn hex0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_twox128() {
        assert_eq!(
            hex::encode(super::twox128(b"System")),
            "26aa394eea5630e07c48ae0c9558cef7"
        );
    }
}
