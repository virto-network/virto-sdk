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
        self.unlocked = self.locked;
        self.unlocked.as_ref().ok_or(Error)
    }

    pub fn lock(&mut self) {
        if let Some(ref mut data) = self.unlocked {
            data.zeroize();
        }
        self.unlocked = None;
    }

    pub(crate) fn replace(&mut self, secret: &[u8]) -> Result<(), Error> {
        let replacement: [u8; N] = secret.try_into().map_err(|_| Error)?;
        if let Some(ref mut locked) = self.locked {
            locked.zeroize();
        }
        self.lock();
        self.locked = Some(replacement);
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        if let Some(ref mut locked) = self.locked {
            locked.zeroize();
        }
        self.locked = None;
        self.lock();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KeyStore, MutableKeyStore};

    #[test]
    fn repeated_upsert_replaces_one_secret_and_delete_forgets_it() {
        let mut store = Simple::<(), 32> {
            locked: Some([1; 32]),
            unlocked: None,
            _phantom: PhantomData,
        };
        MutableKeyStore::upsert(&mut store, &[2; 32]).unwrap();
        MutableKeyStore::upsert(&mut store, &[3; 32]).unwrap();
        assert_eq!(KeyStore::unlock(&mut store).unwrap(), &[3; 32]);

        MutableKeyStore::delete(&mut store).unwrap();
        assert!(KeyStore::unlock(&mut store).is_err());
    }

    #[test]
    fn failed_rotation_preserves_existing_secret() {
        let mut store = Simple::<(), 32> {
            locked: Some([1; 32]),
            unlocked: None,
            _phantom: PhantomData,
        };
        assert!(MutableKeyStore::upsert(&mut store, &[9; 31]).is_err());
        assert_eq!(KeyStore::unlock(&mut store).unwrap(), &[1; 32]);
    }
}
