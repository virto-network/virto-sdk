use crate::vault::{
    utils::{DerivedSigner, RootAccount},
    Vault,
};
use core::marker::PhantomData;
use zeroize::Zeroize;

/// A vault that holds secrets in memory
pub struct Simple<S, const N: usize = 32> {
    locked: Option<[u8; N]>,
    unlocked: Option<[u8; N]>,
    _phantom: PhantomData<S>,
}

impl<S, const N: usize> Simple<S, N> {
    /// A vault with a random seed, once dropped the vault can't be restored.
    #[cfg(feature = "rand")]
    pub fn generate<R>(rng: &mut R) -> Self
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        Simple {
            locked: Some(crate::util::random_bytes::<_, N>(rng)),
            unlocked: None,
            _phantom: Default::default(),
        }
    }

    #[cfg(all(feature = "rand", feature = "mnemonic"))]
    pub fn generate_with_phrase<R>(rng: &mut R) -> (Self, mnemonic::Mnemonic)
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let phrase = crate::util::gen_phrase(rng, Default::default());
        (Self::from_phrase(&phrase), phrase)
    }

    #[cfg(feature = "mnemonic")]
    pub fn from_phrase(phrase: impl AsRef<str>) -> Self {
        mnemonic::Mnemonic::validate(phrase.as_ref()).expect("its a valid mnemonic");
        let mnemonic = mnemonic::Mnemonic::from_phrase(phrase.as_ref()).expect("its a valid mnemonic");
        let raw_entropy = mnemonic.entropy();

        Simple {
            locked: Some(raw_entropy.try_into().expect("its a valid entropy")),
            unlocked: None,
            _phantom: Default::default(),
        }
    }

    pub fn lock(&mut self) {
        if let Some(ref mut data) = self.unlocked {
            data.zeroize();
        }
        self.unlocked = None;
    }

    fn get_key(&self, path: Option<&str>) -> Result<DerivedSigner, Error> {
        if let Some(entropy) = self.unlocked {
            let seed = crate::substrate_seed(&entropy, "");
            let root = RootAccount::from_bytes(&*seed).ok_or(Error)?;
            let path = path.unwrap_or("//default");
            let pair = root.derive(path);
            Ok(DerivedSigner::new(pair, path))
        } else {
            Err(Error)
        }
    }
}

impl<S, const N: usize> Drop for Simple<S, N> {
    fn drop(&mut self) {
        if let Some(ref mut data) = self.locked {
            data.zeroize();
        }
        if let Some(ref mut data) = self.unlocked {
            data.zeroize();
        }
    }
}

#[derive(Debug)]
pub struct Error;
impl core::fmt::Display for Error {
    fn fmt(&self, _f: &mut core::fmt::Formatter) -> core::fmt::Result {
        Ok(())
    }
}
#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl<S: AsRef<str>, const N: usize> Vault for Simple<S, N> {
    type Credentials = ();
    type Error = Error;
    type Id = Option<S>;
    type Signer = DerivedSigner;

    async fn unlock(
        &mut self,
        path: Self::Id,
        _creds: impl Into<Self::Credentials>,
    ) -> Result<Self::Signer, Self::Error> {
        self.unlocked = self.locked.clone();
        let path = path.as_ref().map(|x| x.as_ref());
        self.get_key(path)
    }
}
