use core::fmt::Debug;

pub use derive::Derive;

type Bytes<const N: usize> = [u8; N];

/// A key pair with a public key
pub trait Pair: Signer + Derive {
    type Seed: Seed;
    type Public: Public;

    fn from_bytes(seed: &[u8]) -> Self
    where
        Self: Sized;

    fn public(&self) -> Self::Public;
}

// NOTE: seed could be just a type alias for an array with generic length
// https://github.com/rust-lang/rust/issues/60551
pub trait Seed: AsRef<[u8]> {}
impl<const N: usize> Seed for Bytes<N> {}

pub trait Public: AsRef<[u8]> + Debug {}
impl<const N: usize> Public for Bytes<N> {}

pub trait Signature: AsRef<[u8]> {}
impl<const N: usize> Signature for Bytes<N> {}

/// Something that can sign messages
pub trait Signer {
    type Signature: Signature;
    fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature;
}

/// Wrappers to represent any supported key pair.
pub mod any {
    use super::{Public, Seed, Signature};
    use core::fmt;

    #[derive(Debug)]
    pub enum Pair {
        #[cfg(feature = "sr25519")]
        Sr25519(super::sr25519::Pair),
        #[cfg(not(feature = "sr25519"))]
        _None,
    }

    impl super::Pair for Pair {
        type Seed = AnySeed;
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
            todo!()
        }
    }

    impl super::Derive for Pair {
        type Pair = Pair;

        fn derive(&self, _path: &str) -> Self::Pair
        where
            Self: Sized,
        {
            todo!()
        }
    }

    impl super::Signer for Pair {
        type Signature = AnySignature;

        fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(p) => p.sign_msg(msg).into(),
                #[cfg(not(feature = "sr25519"))]
                _ => unreachable!(),
            }
        }
    }

    #[derive(Debug)]
    pub enum AnySeed {}

    impl AsRef<[u8]> for AnySeed {
        fn as_ref(&self) -> &[u8] {
            todo!()
        }
    }

    impl Seed for AnySeed {}

    #[derive(Debug)]
    pub enum AnyPublic {
        #[cfg(feature = "sr25519")]
        Sr25519(super::Bytes<{ super::sr25519::SEED_LEN }>),
        #[cfg(not(feature = "sr25519"))]
        _None,
    }

    impl AsRef<[u8]> for AnyPublic {
        fn as_ref(&self) -> &[u8] {
            todo!()
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

    #[derive(Debug)]
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
    use super::{Bytes, Derive, Signer};
    use schnorrkel::{signing_context, ExpansionMode, MiniSecretKey, MINI_SECRET_KEY_LENGTH};

    pub use schnorrkel::Keypair as Pair;
    pub(super) const SEED_LEN: usize = MINI_SECRET_KEY_LENGTH;
    pub(super) const SIG_LEN: usize = 64;
    pub type Seed = Bytes<SEED_LEN>;
    pub type Public = Bytes<32>;
    pub type Signature = Bytes<64>;
    const SIGNING_CTX: &[u8] = b"substrate";

    impl super::Pair for Pair {
        type Seed = Seed;
        type Public = Public;

        fn from_bytes(bytes: &[u8]) -> Self {
            let minikey = MiniSecretKey::from_bytes(&bytes).expect("Seed size");
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
    }

    impl Derive for Pair {
        type Pair = Self;

        fn derive(&self, _path: &str) -> Self
        where
            Self: Sized,
        {
            todo!()
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

    pub(super) fn parse_substrate_junctions(
        path: &str,
    ) -> impl Iterator<Item = (Bytes<JUNCTION_LEN>, bool)> + '_ {
        path.split_inclusive("/")
            .flat_map(|s| if s == "/" { "" } else { s }.split("/")) // "//Alice//Bob" -> ["","","Alice","","","Bob"]
            .scan(0u8, |x, part| {
                Some(if part.is_empty() {
                    *x += 1;
                    None
                } else {
                    let hard = *x > 1;
                    *x = 0;
                    Some((encoded_junction(part), hard))
                })
            })
            .filter_map(identity)
    }

    fn encoded_junction(part: &str) -> Bytes<JUNCTION_LEN> {
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
    mod test {
        use super::*;

        #[test]
        fn substrate_junctions() {
            let path = "//Alice//Bob/123//loremipsumdolor";
            let out: Vec<_> = parse_substrate_junctions(path).map(|(_, h)| h).collect();
            assert_eq!(out, vec![true, true, false, true]);
        }
    }
}
