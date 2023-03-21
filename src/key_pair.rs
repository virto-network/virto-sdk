use core::fmt::Debug;

pub use derive::Derive;

type Bytes<const N: usize> = [u8; N];

/// A key pair with a public key
pub trait Pair: Signer + Derive {
    type Public: Public;

    fn from_bytes(seed: &[u8]) -> Self
    where
        Self: Sized;

    fn public(&self) -> Self::Public;
}

pub trait Public: AsRef<[u8]> + Debug {}
impl<const N: usize> Public for Bytes<N> {}

pub trait Signature: AsRef<[u8]> + Debug + PartialEq {}
impl<const N: usize> Signature for Bytes<N> {}

/// Something that can sign messages
pub trait Signer {
    type Signature: Signature;
    fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature;
    fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool;
}

/// Wrappers to represent any supported key pair.
pub mod any {
    use super::{Public, Signature};
    use core::fmt;

    #[derive(Debug)]
    pub enum Pair {
        #[cfg(feature = "sr25519")]
        Sr25519(super::sr25519::Pair),
        #[cfg(not(feature = "sr25519"))]
        _None,
    }

    impl super::Pair for Pair {
        type Public = AnyPublic;

        fn from_bytes(seed: &[u8]) -> Self
        where
            Self: Sized,
        {
            #[cfg(feature = "sr25519")]
            let p = Self::Sr25519(<super::sr25519::Pair as super::Pair>::from_bytes(seed));
            #[cfg(not(feature = "sr25519"))]
            let p = Self::_None;
            p
        }

        fn public(&self) -> Self::Public {
            match self {
                Pair::Sr25519(p) => AnyPublic::Sr25519(p.public()),
            }
        }
    }

    impl super::Derive for Pair {
        type Pair = Pair;

        fn derive(&self, path: &str) -> Self::Pair
        where
            Self: Sized,
        {
            match self {
                Pair::Sr25519(kp) => Pair::Sr25519(kp.derive(path)),
            }
        }
    }

    #[cfg(feature = "sr25519")]
    impl From<super::sr25519::Pair> for Pair {
        fn from(p: super::sr25519::Pair) -> Self {
            Self::Sr25519(p)
        }
    }

    impl super::Signer for Pair {
        type Signature = AnySignature;

        fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(p) => p.sign_msg(msg).into(),
            }
        }

        fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool {
            match self {
                Pair::Sr25519(p) => super::Signer::verify(p, msg, sig),
            }
        }
    }

    #[derive(Debug)]
    pub enum AnyPublic {
        #[cfg(feature = "sr25519")]
        Sr25519(super::Bytes<{ super::sr25519::SEED_LEN }>),
        #[cfg(not(feature = "sr25519"))]
        _None,
    }

    impl AsRef<[u8]> for AnyPublic {
        fn as_ref(&self) -> &[u8] {
            match self {
                AnyPublic::Sr25519(p) => p.as_ref(),
            }
        }
    }

    impl fmt::Display for AnyPublic {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            for b in self.as_ref() {
                write!(f, "{:02X}", b)?;
            }
            Ok(())
        }
    }
    impl Public for AnyPublic {}

    #[derive(Debug, PartialEq)]
    pub enum AnySignature {
        #[cfg(feature = "sr25519")]
        Sr25519(super::Bytes<{ super::sr25519::SIG_LEN }>),

        #[cfg(not(feature = "sr25519"))]
        _None,
    }
    impl AsRef<[u8]> for AnySignature {
        fn as_ref(&self) -> &[u8] {
            match self {
                #[cfg(feature = "sr25519")]
                AnySignature::Sr25519(s) => s.as_ref(),
            }
        }
    }
    #[cfg(feature = "sr25519")]
    impl From<super::sr25519::Signature> for AnySignature {
        fn from(s: super::sr25519::Signature) -> Self {
            AnySignature::Sr25519(s)
        }
    }

    impl Signature for AnySignature {}
}

#[cfg(feature = "sr25519")]
pub mod sr25519 {
    use super::{derive::Junction, Bytes, Derive, Signer};
    use schnorrkel::{
        derive::{ChainCode, Derivation},
        signing_context, ExpansionMode, MiniSecretKey, SecretKey, MINI_SECRET_KEY_LENGTH,
    };

    pub use schnorrkel::Keypair as Pair;
    pub(super) const SEED_LEN: usize = MINI_SECRET_KEY_LENGTH;
    pub(super) const SIG_LEN: usize = 64;
    pub type Seed = Bytes<SEED_LEN>;
    pub type Public = Bytes<32>;
    pub type Signature = Bytes<64>;
    const SIGNING_CTX: &[u8] = b"substrate";

    impl super::Pair for Pair {
        type Public = Public;

        fn from_bytes(bytes: &[u8]) -> Self {
            assert!(bytes.len() >= SEED_LEN);
            let minikey = MiniSecretKey::from_bytes(&bytes[..SEED_LEN]).unwrap();
            minikey.expand_to_keypair(ExpansionMode::Ed25519)
        }

        fn public(&self) -> Self::Public {
            let mut key = [0u8; 32];
            key.copy_from_slice(self.public.as_ref());
            key
        }
    }

    impl Signer for Pair {
        type Signature = Signature;

        fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
            let context = signing_context(SIGNING_CTX);
            self.sign(context.bytes(msg.as_ref())).to_bytes()
        }

        fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool {
            let sig = match schnorrkel::Signature::from_bytes(sig) {
                Ok(s) => s,
                Err(_) => return false,
            };
            self.public
                .verify_simple(SIGNING_CTX, msg.as_ref(), &sig)
                .is_ok()
        }
    }

    impl Derive for Pair {
        type Pair = Self;

        fn derive(&self, path: &str) -> Self
        where
            Self: Sized,
        {
            super::derive::parse_substrate_junctions(path)
                .fold(self.secret.clone(), |key, (part, hard)| {
                    if hard {
                        key.hard_derive_mini_secret_key(Some(ChainCode(part)), &[])
                            .0
                            .expand(ExpansionMode::Ed25519)
                    } else {
                        derive_simple(key, part)
                    }
                })
                .into()
        }
    }

    #[cfg(not(feature = "rand_chacha"))]
    fn derive_simple(key: SecretKey, j: Junction) -> SecretKey {
        key.derived_key_simple(ChainCode(j), &[]).0
    }
    #[cfg(feature = "rand_chacha")]
    fn derive_simple(key: SecretKey, j: Junction) -> SecretKey {
        use rand_core::SeedableRng;
        // As noted in https://docs.rs/schnorrkel/latest/schnorrkel/context/fn.attach_rng.html
        // it's not recommended by should be ok for our simple use cases
        let rng = rand_chacha::ChaChaRng::from_seed([0; 32]);
        key.derived_key_simple_rng(ChainCode(j), &[], rng).0
    }

    #[cfg(test)]
    mod tests {
        use crate::Mnemonic;
        use crate::{util::Pin, Derive, Pair};

        #[test]
        fn derive_substrate_keypair() {
            // "rotate increase color sustain print future moon rigid hunt wild diagram online"
            let seed = b"\x70\x8a\x2b\xe9\x96\xb8\x7d\x1e\x7b\xb2\x3f\x3c\xfa\x9b\xac\x80\x4c\x83\x35\x9e\x30\x85\x98\xb0\xcb\x20\x72\x82\x90\x68\x47\x57";

            for (path, pubkey) in [
                // from subkey
                (
                    "//test",
                    b"\x0a\x04\x17\x5e\x09\x7c\x49\x26\x45\xa9\x8e\x1f\x28\x18\xa3\x95\x07\xb9\xfc\xba\x02\x03\x4d\x24\x4d\x27\xa3\x4d\xd3\xea\x2a\x11",
                ), (
                    "/test",
                    b"\x9e\x75\x15\xf2\x87\x0a\xee\x0c\x54\x5f\x84\x35\x1f\xd4\xed\xd3\xc2\x48\x26\x8d\x2c\xb5\xfd\x97\x88\x55\x12\x10\xb8\x99\x9b\x76",
                ), (
                    "//test//123",
                    b"\x50\xb3\x99\x79\xff\x3b\x54\x7d\x41\x7c\x8e\xda\xe8\xab\x84\x21\x0a\x6d\xef\x64\x14\x3f\x3e\xdc\x46\x7a\xf5\x2a\xf5\x53\x72\x06",
                ), (
                    "//test/123",
                    b"\x7a\x39\xc7\x6b\x2a\x0c\x25\xc7\x37\x92\x0d\x5a\x4c\xc4\x07\x6e\xdd\x7a\xe2\xf0\x48\x99\x9b\x92\x54\xa7\xe6\x11\xcf\xf8\x78\x3a",
                ), (
                    "/test/123",
                    b"\x48\xce\x4b\x7e\x7c\xe5\x87\xf6\xad\x1e\x14\x96\x51\x77\x94\xf1\x28\x82\xb9\xff\x69\xc9\x11\xf7\xda\x7c\x15\x7a\xdc\x9d\x24\x4e",
                ),
            ] {
                let root: super::Pair = Pair::from_bytes(seed);
                let derived = root.derive(path);
                assert_eq!(&derived.public(), pubkey);
            }
        }

        #[test]
        fn derive_keypair_from_phrase() {
            // 0x708a2be996b87d1e7bb23f3cfa9bac804c83359e308598b0cb20728290684757
            let phrase =
                "rotate increase color sustain print future moon rigid hunt wild diagram online";

            for (path, pubkey) in [
                // from subkey
                (
                    "//test",
                    b"\x0a\x04\x17\x5e\x09\x7c\x49\x26\x45\xa9\x8e\x1f\x28\x18\xa3\x95\x07\xb9\xfc\xba\x02\x03\x4d\x24\x4d\x27\xa3\x4d\xd3\xea\x2a\x11",
                ), (
                    "/test",
                    b"\x9e\x75\x15\xf2\x87\x0a\xee\x0c\x54\x5f\x84\x35\x1f\xd4\xed\xd3\xc2\x48\x26\x8d\x2c\xb5\xfd\x97\x88\x55\x12\x10\xb8\x99\x9b\x76",
                ), (
                    "//test//123",
                    b"\x50\xb3\x99\x79\xff\x3b\x54\x7d\x41\x7c\x8e\xda\xe8\xab\x84\x21\x0a\x6d\xef\x64\x14\x3f\x3e\xdc\x46\x7a\xf5\x2a\xf5\x53\x72\x06",
                ), (
                    "//test/123",
                    b"\x7a\x39\xc7\x6b\x2a\x0c\x25\xc7\x37\x92\x0d\x5a\x4c\xc4\x07\x6e\xdd\x7a\xe2\xf0\x48\x99\x9b\x92\x54\xa7\xe6\x11\xcf\xf8\x78\x3a",
                ), (
                    "/test/123",
                    b"\x48\xce\x4b\x7e\x7c\xe5\x87\xf6\xad\x1e\x14\x96\x51\x77\x94\xf1\x28\x82\xb9\xff\x69\xc9\x11\xf7\xda\x7c\x15\x7a\xdc\x9d\x24\x4e",
                ),
            ] {
                let phrase = Mnemonic::from_phrase(phrase).unwrap();
                let seed = Pin::from("").protect::<64>(&phrase.entropy());
                let root: super::Pair = Pair::from_bytes(&seed);
                let derived = root.derive(path);
                assert_eq!(&derived.public(), pubkey);
            }
        }
    }
}

mod derive {
    use core::convert::identity;

    use super::Bytes;

    /// Something to derive key pairs form
    pub trait Derive {
        type Pair: super::Signer;

        fn derive(&self, path: &str) -> Self::Pair
        where
            Self: Sized;
    }

    const JUNCTION_LEN: usize = 32;
    pub(super) type Junction = Bytes<JUNCTION_LEN>;

    pub(super) fn parse_substrate_junctions(
        path: &str,
    ) -> impl Iterator<Item = (Junction, bool)> + '_ {
        path.split_inclusive("/")
            .flat_map(|s| if s == "/" { "" } else { s }.split("/")) // "//Alice//Bob" -> ["","","Alice","","","Bob"]
            .scan(0u8, |j, part| {
                Some(if part.is_empty() {
                    *j += 1;
                    None
                } else {
                    let hard = *j > 1;
                    *j = 0;
                    Some((encoded_junction(part), hard))
                })
            })
            .filter_map(identity)
    }

    fn encoded_junction(part: &str) -> Junction {
        let mut code = [0; JUNCTION_LEN];
        if let Ok(n) = part.parse::<u64>() {
            code[..8].copy_from_slice(&n.to_le_bytes());
        } else {
            let len = part.len().min(JUNCTION_LEN - 1);
            code[0] = (len as u8) << 2;
            code[1..len + 1].copy_from_slice(&part[..len].as_bytes());
        }
        code
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn substrate_junctions() {
            let path = "//Alice//Bob/123//loremipsumdolor";
            let out: Vec<_> = parse_substrate_junctions(path).map(|(_, h)| h).collect();
            assert_eq!(out, vec![true, true, false, true]);
        }
    }
}
