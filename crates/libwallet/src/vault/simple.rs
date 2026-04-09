use core::marker::PhantomData;
use zeroize::Zeroize;

/// A simple in-memory key store holding raw entropy.
/// Not a vault by itself — wrap with a chain-specific vault like
/// `Substrate<Simple<..>>` to produce usable signers.
pub struct Simple<S, const N: usize = 32> {
    locked: Option<[u8; N]>,
    unlocked: Option<[u8; N]>,
    _phantom: PhantomData<S>,
}

impl<S, const N: usize> Simple<S, N> {
    /// A key store with a random seed, once dropped it can't be restored.
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
    pub fn generate_with_phrase<R>(rng: &mut R) -> Result<(Self, mnemonic::Mnemonic), Error>
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let phrase = crate::util::gen_phrase(rng, Default::default());
        Ok((Self::from_phrase(&phrase)?, phrase))
    }

    #[cfg(feature = "mnemonic")]
    pub fn from_phrase(phrase: impl AsRef<str>) -> Result<Self, Error> {
        mnemonic::Mnemonic::validate(phrase.as_ref()).map_err(|_| Error)?;
        let mnemonic = mnemonic::Mnemonic::from_phrase(phrase.as_ref()).map_err(|_| Error)?;
        let raw_entropy = mnemonic.entropy();

        Ok(Simple {
            locked: Some(raw_entropy.try_into().map_err(|_| Error)?),
            unlocked: None,
            _phantom: Default::default(),
        })
    }

    /// Unlock the key store, making the raw entropy available.
    pub fn unlock(&mut self) -> Result<&[u8; N], Error> {
        self.unlocked = self.locked.clone();
        self.unlocked.as_ref().ok_or(Error)
    }

    pub fn lock(&mut self) {
        if let Some(ref mut data) = self.unlocked {
            data.zeroize();
        }
        self.unlocked = None;
    }

    /// Access the raw entropy (must be unlocked).
    pub fn entropy(&self) -> Option<&[u8; N]> {
        self.unlocked.as_ref()
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
