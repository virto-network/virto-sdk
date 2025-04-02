use core::{iter, ops};
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
    let phrase = mnemonic::Mnemonic::from_entropy_in(lang, seed.as_ref()).expect("seed valid");
    phrase
}

/// A simple pin credential that can be used to add some
/// extra level of protection to seeds stored in vaults
#[derive(Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pin(u16);

macro_rules! seed_from_entropy {
    ($seed: ident, $pin: expr) => {
        #[cfg(feature = "util_pin")]
        let protected_seed = $pin.protect::<64>($seed);
        #[cfg(feature = "util_pin")]
        let $seed = &protected_seed;
        #[cfg(not(feature = "util_pin"))]
        let _ = &$pin; // use the variable to avoid warning
    };
}

pub(crate) use seed_from_entropy;

impl Pin {
    const LEN: usize = 4;

    #[cfg(feature = "util_pin")]
    pub fn protect<const S: usize>(&self, data: &[u8]) -> [u8; S] {
        use hmac::Hmac;
        use pbkdf2::pbkdf2;
        use sha2::Sha512;

        let salt = {
            let mut s = [0; 10];
            s.copy_from_slice(b"mnemonic\0\0");
            let [b1, b2] = self.to_le_bytes();
            s[8] = b1;
            s[9] = b2;
            s
        };
        let mut seed = [0; S];
        // using same hashing strategy as Substrate to have some compatibility
        // when pin is 0(no pin) we produce the same addresses
        let len = if self.eq(&0) {
            salt.len() - 2
        } else {
            salt.len()
        };
        pbkdf2::<Hmac<Sha512>>(data, &salt[..len], 2048, &mut seed);
        seed
    }
}

// Use 4 chars long hex string as pin. i.e. "ABCD", "1234"
impl From<&str> for Pin {
    fn from(s: &str) -> Self {
        let l = s.len().min(Pin::LEN);
        let chars = s
            .chars()
            .take(l)
            .chain(iter::repeat('0').take(Pin::LEN - l));
        Pin(chars
            .map(|c| c.to_digit(16).unwrap_or(0))
            .enumerate()
            .fold(0, |pin, (i, d)| {
                pin | ((d as u16) << ((Pin::LEN - 1 - i) * Pin::LEN))
            }))
    }
}

impl<'a> From<Option<&'a str>> for Pin {
    fn from(p: Option<&'a str>) -> Self {
        p.unwrap_or("").into()
    }
}

impl From<()> for Pin {
    fn from(_: ()) -> Self {
        Self(0)
    }
}

impl From<u16> for Pin {
    fn from(n: u16) -> Self {
        Self(n)
    }
}

impl ops::Deref for Pin {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[test]
fn pin_parsing() {
    for (s, expected) in [
        ("0000", 0),
        // we only take the first 4 characters and ignore the rest
        ("0000001", 0),
        // non hex chars are ignored and defaulted to 0, here a,d are kept
        ("zasdasjgkadg", 0x0A0D),
        ("ABCD", 0xABCD),
        ("1000", 0x1000),
        ("000F", 0x000F),
        ("FFFF", 0xFFFF),
    ] {
        let pin = Pin::from(s);
        assert_eq!(
            *pin, expected,
            "(input:\"{}\", l:{:X} == r:{:X})",
            s, *pin, expected
        );
    }
}
