use crate::util::{seed_from_entropy, Pin};
use crate::{RootAccount, Vault};

/// A vault that holds secrets in memory
pub struct Simple {
    locked: Option<Vec<u8>>,
    unlocked: Option<Vec<u8>>,
}

impl Simple {
    /// A vault with a random seed, once dropped the the vault can't be restored
    ///
    /// ```
    /// # use libwallet::{vault, Error, Derive, Pair, Vault};
    /// # type Result = std::result::Result<(), <vault::Simple as Vault>::Error>;
    /// # #[async_std::main] async fn main() -> Result {
    /// let mut vault = vault::Simple::generate(&mut rand_core::OsRng);
    /// let root = vault.unlock(None, |root| {
    ///     println!("{}", root.derive("//default").public());
    /// }).await?;
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "rand")]
    pub fn generate<R>(rng: &mut R) -> Self
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let seed = &crate::util::random_bytes::<_, 32>(rng);

        Simple {
            locked: Some(seed.to_vec()),
            unlocked: None,
        }
    }

    #[cfg(all(feature = "rand", feature = "mnemonic"))]
    pub fn generate_with_phrase<R>(rng: &mut R) -> (Self, mnemonic::Mnemonic)
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let phrase = crate::util::gen_phrase(rng, Default::default());

        (
            Simple {
                locked: Some(phrase.entropy().to_vec()),
                unlocked: None,
            },
            phrase,
        )
    }

    #[cfg(feature = "mnemonic")]
    // Provide your own seed
    pub fn from_phrase(phrase: impl AsRef<str>) -> Self {
        let phrase = phrase
            .as_ref()
            .parse::<mnemonic::Mnemonic>()
            .expect("mnemonic");

        Simple {
            locked: Some(phrase.entropy().to_vec()),
            unlocked: None,
        }
    }

    fn get_key(&self, pin: Pin) -> Result<RootAccount, Error> {
        if let Some(entropy) = &self.unlocked {
            let seed = entropy.as_slice();
            seed_from_entropy!(seed, pin);
            Ok(RootAccount::from_bytes(seed))
        } else {
            Err(Error)
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

impl Vault for Simple {
    type Credentials = Option<Pin>;
    type Error = Error;

    async fn unlock<T>(
        &mut self,
        credentials: impl Into<Self::Credentials>,
        mut cb: impl FnMut(&RootAccount) -> T,
    ) -> Result<T, Self::Error> {
        self.unlocked = self.locked.take();
        let pin = credentials.into();
        let root_account = &self.get_key(pin.unwrap_or_default())?;
        Ok(cb(root_account))
    }
}
