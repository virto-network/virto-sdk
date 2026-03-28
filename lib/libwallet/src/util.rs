#[cfg(feature = "mnemonic")]
use mnemonic::{Language, Mnemonic};

#[cfg(feature = "rand")]
pub fn random_bytes<R, const S: usize>(rng: &mut R) -> [u8; S]
where
    R: rand_core::CryptoRng + rand_core::RngCore,
{
    let mut bytes = [0u8; S];
    rng.fill_bytes(&mut bytes);
    bytes
}

#[cfg(feature = "rand")]
pub fn gen_phrase<R>(rng: &mut R, lang: mnemonic::Language) -> mnemonic::Mnemonic
where
    R: rand_core::CryptoRng + rand_core::RngCore,
{
    let seed = random_bytes::<_, 32>(rng);
    mnemonic::Mnemonic::from_entropy_in(lang, seed.as_ref()).expect("seed valid")
}

const MAX_PIN_LEN: usize = 64;

/// A passphrase credential used to protect seeds stored in vaults.
///
/// Accepts inputs from short numeric PINs to full passphrases.
/// The raw bytes are fed into PBKDF2-HMAC-SHA512 (210,000 rounds) as part
/// of the salt. Longer passphrases provide proportionally more security.
///
/// When empty, produces Substrate-compatible addresses (salt = `"mnemonic"`).
#[derive(Clone)]
pub struct Pin {
    #[allow(dead_code)] // used by protect() behind util_pin feature
    buf: [u8; MAX_PIN_LEN],
    len: u8,
}

impl Default for Pin {
    fn default() -> Self {
        Pin { buf: [0u8; MAX_PIN_LEN], len: 0 }
    }
}

macro_rules! seed_from_entropy {
    ($seed: ident, $pin: expr) => {
        #[cfg(feature = "util_pin")]
        let protected_seed = zeroize::Zeroizing::new($pin.protect::<64>($seed));
        #[cfg(feature = "util_pin")]
        let $seed: &[u8] = &*protected_seed;
        #[cfg(not(feature = "util_pin"))]
        let _ = &$pin; // use the variable to avoid warning
    };
}

pub(crate) use seed_from_entropy;

#[cfg(feature = "util_pin")]
const PBKDF2_ROUNDS: u32 = 210_000;

impl Pin {
    /// Returns true if the pin is empty (no passphrase set).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[allow(dead_code)] // used by protect() behind util_pin and by tests
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len as usize]
    }

    #[cfg(feature = "util_pin")]
    pub fn protect<const S: usize>(&self, data: &[u8]) -> [u8; S] {
        use hmac::Hmac;
        use pbkdf2::pbkdf2;
        use sha2::Sha512;

        let mut seed = [0; S];
        if self.is_empty() {
            // Substrate-compatible: salt is just "mnemonic", same iteration count
            pbkdf2::<Hmac<Sha512>>(data, b"mnemonic", 2048, &mut seed);
        } else {
            // Build salt: "mnemonic" prefix + raw passphrase bytes
            let pin_bytes = self.as_bytes();
            let salt_len = 8 + pin_bytes.len();
            // Stack-allocate salt: "mnemonic" + up to 64 bytes of passphrase
            let mut salt = [0u8; 8 + MAX_PIN_LEN];
            salt[..8].copy_from_slice(b"mnemonic");
            salt[8..salt_len].copy_from_slice(pin_bytes);
            pbkdf2::<Hmac<Sha512>>(data, &salt[..salt_len], PBKDF2_ROUNDS, &mut seed);
        }
        seed
    }
}

impl From<&str> for Pin {
    fn from(s: &str) -> Self {
        let bytes = s.as_bytes();
        let len = bytes.len().min(MAX_PIN_LEN);
        let mut buf = [0u8; MAX_PIN_LEN];
        buf[..len].copy_from_slice(&bytes[..len]);
        Pin { buf, len: len as u8 }
    }
}

impl<'a> From<Option<&'a str>> for Pin {
    fn from(p: Option<&'a str>) -> Self {
        p.unwrap_or("").into()
    }
}

impl From<()> for Pin {
    fn from(_: ()) -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::Pin;

    #[test]
    fn empty_pin() {
        let pin = Pin::from("");
        assert!(pin.is_empty());
    }

    #[test]
    fn short_pin() {
        let pin = Pin::from("1234");
        assert!(!pin.is_empty());
        assert_eq!(pin.as_bytes(), b"1234");
    }

    #[test]
    fn long_passphrase() {
        let pin = Pin::from("correct horse battery staple");
        assert_eq!(pin.as_bytes(), b"correct horse battery staple");
    }

    #[test]
    fn truncates_at_max() {
        let long = "a]".repeat(64); // 128 chars
        let pin = Pin::from(long.as_str());
        assert_eq!(pin.as_bytes().len(), 64);
    }
}
