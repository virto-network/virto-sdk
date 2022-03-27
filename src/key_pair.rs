use core::{
    convert::{TryFrom, TryInto},
    fmt::Debug,
};

// pub type Signature<const S: usize> = [u8; S];

pub trait Pair: Derive {
    const SEED_LEN: usize;
    const SIG_LEN: usize;

    type Public: AsRef<[u8]> + Derive + Debug;
    type Seed: Seed;
    type Signature: AsRef<[u8]>;

    fn from_bytes(seed: &[u8]) -> (Self, Self::Seed)
    where
        Self: Sized;

    fn public(&self) -> Self::Public;
    fn sign(&self, msg: &[u8]) -> Self::Signature;

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
impl<const N: usize> Seed for [u8; N] {}

pub trait Derive {
    fn derive(&self, path: &str) -> Option<Self>
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

#[cfg(feature = "sr25519")]
pub mod sr25519 {
    use super::Derive;
    use schnorrkel::{signing_context, ExpansionMode, MiniSecretKey, MINI_SECRET_KEY_LENGTH};

    pub use schnorrkel::Keypair as Pair;

    const SIGNING_CTX: &[u8] = b"substrate";

    #[derive(Debug)]
    pub struct Public([u8; 32]);

    // pub type Seed = [u8; Pair::SEED_LEN];
    // pub type Signature = [u8; Pair::SIG_LEN];

    impl Derive for Public {
        fn derive(&self, _path: &str) -> Option<Self> {
            todo!()
        }
    }

    impl AsRef<[u8]> for Public {
        fn as_ref(&self) -> &[u8] {
            self.0.as_ref()
        }
    }

    impl super::Pair for Pair {
        const SEED_LEN: usize = MINI_SECRET_KEY_LENGTH;
        const SIG_LEN: usize = 64;

        type Public = Public;
        type Seed = [u8; Self::SEED_LEN];
        type Signature = [u8; Self::SIG_LEN];

        fn from_bytes(bytes: &[u8]) -> (Self, Self::Seed) {
            let seed = Self::seed_from_bytes(bytes);
            let minikey = MiniSecretKey::from_bytes(&seed).expect("Seed size");
            (minikey.expand_to_keypair(ExpansionMode::Ed25519), seed)
        }

        fn public(&self) -> Self::Public {
            let mut key = [0u8; 32];
            key.copy_from_slice(self.public.as_ref());
            Public(key)
        }

        fn sign(&self, msg: &[u8]) -> Self::Signature {
            let context = signing_context(SIGNING_CTX);
            self.sign(context.bytes(msg)).to_bytes()
        }

        #[cfg(feature = "std")]
        fn generate() -> (Self, Self::Seed)
        where
            Self: Sized,
        {
            let seed = super::util::generate::<{ Self::SEED_LEN }>();
            <Self as super::Pair>::from_bytes(&seed)
        }
    }

    impl Derive for Pair {
        fn derive(&self, _path: &str) -> Option<Self>
        where
            Self: Sized,
        {
            todo!()
        }
    }
}
