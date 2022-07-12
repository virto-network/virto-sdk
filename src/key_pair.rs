use core::fmt::Debug;

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

/// Something to derive key pairs form
pub trait Derive {
    type Pair: Signer;

    fn derive(&self, path: &str) -> Self::Pair
    where
        Self: Sized;
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
