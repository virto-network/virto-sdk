use core::{
    convert::{TryFrom, TryInto},
    fmt::Debug,
};

type Bytes<const N: usize> = [u8; N];

pub trait Pair: Signer + Derive {
    type Seed: Seed;
    type Public: Public;

    fn from_bytes(seed: &[u8]) -> (Self, Self::Seed)
    where
        Self: Sized;

    fn public(&self) -> Self::Public;

    #[cfg(feature = "std")]
    fn generate() -> (Self, Self::Seed)
    where
        Self: Sized;

    #[cfg(feature = "mnemonic")]
    fn generate_with_phrase(lang: mnemonic::Language) -> (Self, crate::Mnemonic)
    where
        Self: Sized,
    {
        let (pair, seed) = Self::generate();
        let phrase = mnemonic::Mnemonic::from_entropy_in(lang, seed.as_ref()).expect("seed valid");
        (pair, phrase)
    }

    fn seed_from_bytes(bytes: &[u8]) -> Self::Seed {
        bytes.try_into().map_err(|_| ()).expect("Seed size")
    }
}

// seed could be just a type alias for an array with generic length
// https://github.com/rust-lang/rust/issues/60551
pub trait Seed: AsRef<[u8]> + for<'a> TryFrom<&'a [u8]> {}
impl<const N: usize> Seed for Bytes<N> {}

pub trait Public: AsRef<[u8]> + Debug {}
impl<const N: usize> Public for Bytes<N> {}

pub trait Signature: AsRef<[u8]> {}
impl<const N: usize> Signature for Bytes<N> {}

pub trait Signer {
    type Signature: Signature;
    fn sign<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature;
}

pub trait Derive {
    type Pair: Signer;

    fn derive(&self, path: &str) -> Option<Self::Pair>
    where
        Self: Sized;
}

pub mod util {
    #[cfg(feature = "std")]
    pub fn generate<const S: usize>() -> [u8; S] {
        generate_with::<_, S>(&mut rand_core::OsRng)
    }

    #[cfg(feature = "rand")]
    pub fn generate_with<R, const S: usize>(rng: &mut R) -> [u8; S]
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let mut bytes = [0u8; S];
        rng.fill_bytes(&mut bytes);
        bytes
    }
}

pub mod any {
    use core::{convert::TryFrom, fmt};

    use crate::{Public, Seed, Signature};

    pub enum Pair {
        #[cfg(feature = "sr25519")]
        Sr25519(super::sr25519::Pair),
    }

    impl super::Pair for Pair {
        type Seed = AnySeed;
        type Public = AnyPublic;

        fn from_bytes(seed: &[u8]) -> (Self, Self::Seed)
        where
            Self: Sized,
        {
            todo!()
        }

        fn public(&self) -> Self::Public {
            todo!()
        }

        #[cfg(feature = "std")]
        fn generate() -> (Self, Self::Seed)
        where
            Self: Sized,
        {
            todo!()
        }
    }

    impl super::Derive for Pair {
        type Pair = Pair;

        fn derive(&self, path: &str) -> Option<Self::Pair>
        where
            Self: Sized,
        {
            todo!()
        }
    }

    impl super::Signer for Pair {
        type Signature = AnySignature;

        fn sign<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
            todo!()
        }
    }

    #[derive(Debug)]
    pub enum AnySeed {}

    impl AsRef<[u8]> for AnySeed {
        fn as_ref(&self) -> &[u8] {
            todo!()
        }
    }
    impl TryFrom<&[u8]> for AnySeed {
        type Error = ();
        fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
            todo!()
        }
    }
    impl Seed for AnySeed {}

    #[derive(Debug)]
    pub enum AnyPublic {}

    impl AsRef<[u8]> for AnyPublic {
        fn as_ref(&self) -> &[u8] {
            todo!()
        }
    }
    impl TryFrom<&[u8]> for AnyPublic {
        type Error = ();
        fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
            todo!()
        }
    }
    impl fmt::Display for AnyPublic {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            todo!()
        }
    }
    impl Public for AnyPublic {}

    #[derive(Debug)]
    pub enum AnySignature {
        #[cfg(feature = "sr25519")]
        Sr25519,
    }
    impl Default for AnySignature {
        fn default() -> Self {
            #[cfg(feature = "sr25519")]
            Self::Sr25519
        }
    }
    impl AsRef<[u8]> for AnySignature {
        fn as_ref(&self) -> &[u8] {
            todo!()
        }
    }
    impl TryFrom<&[u8]> for AnySignature {
        type Error = ();
        fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
            todo!()
        }
    }
    impl Signature for AnySignature {}
}

#[cfg(feature = "sr25519")]
pub mod sr25519 {
    use super::{Bytes, Derive, Signer};
    use schnorrkel::{signing_context, ExpansionMode, MiniSecretKey, MINI_SECRET_KEY_LENGTH};

    pub use schnorrkel::Keypair as Pair;
    pub const SEED_LEN: usize = MINI_SECRET_KEY_LENGTH;

    const SIGNING_CTX: &[u8] = b"substrate";

    impl super::Pair for Pair {
        type Seed = Bytes<SEED_LEN>;
        type Public = Bytes<32>;

        fn from_bytes(bytes: &[u8]) -> (Self, Self::Seed) {
            let seed = Self::seed_from_bytes(bytes);
            let minikey = MiniSecretKey::from_bytes(&seed).expect("Seed size");
            (minikey.expand_to_keypair(ExpansionMode::Ed25519), seed)
        }

        fn public(&self) -> Self::Public {
            let mut key = [0u8; 32];
            key.copy_from_slice(self.public.as_ref());
            key
        }

        #[cfg(feature = "std")]
        fn generate() -> (Self, Self::Seed)
        where
            Self: Sized,
        {
            let seed = super::util::generate::<{ SEED_LEN }>();
            <Self as super::Pair>::from_bytes(&seed)
        }
    }

    impl Signer for Pair {
        type Signature = Bytes<64>;

        fn sign<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
            let context = signing_context(SIGNING_CTX);
            self.sign(context.bytes(msg.as_ref())).to_bytes()
        }
    }

    impl Derive for Pair {
        type Pair = Self;

        fn derive(&self, _path: &str) -> Option<Self>
        where
            Self: Sized,
        {
            todo!()
        }
    }
}
