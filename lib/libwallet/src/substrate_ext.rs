use crate::Network;

impl From<&str> for Network {
    fn from(s: &str) -> Self {
        match s {
            "polkadot" => Network::Substrate(0),
            "kusama" => Network::Substrate(2),
            "karura" => Network::Substrate(8),
            "substrate" | _ => Network::Substrate(42),
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
        // salt = "mnemonic" + passphrase bytes (BIP39 spec)
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
