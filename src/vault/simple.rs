use crate::util::Pin;
use crate::{RootAccount, Vault};

/// A vault that holds secrets in memory
pub struct Simple {
    locked: Option<RootAccount>,
    unlocked: Option<RootAccount>,
}

impl Simple {
    /// A vault with a random seed, once dropped the the vault can't be restored
    ///
    /// ```
    /// # use libwallet::{vault, Error, Derive, Pair, Vault};
    /// # type Result = std::result::Result<(), <vault::Simple as Vault>::Error>;
    /// # #[async_std::main] async fn main() -> Result {
    /// let mut vault = vault::Simple::generate(&mut rand_core::OsRng);
    /// let root = vault.unlock(&(), |root| {
    ///     println!("{}", root.derive("//default").public());
    /// }).await?;
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "rand")]
    pub fn generate<R>(rng: &mut R) -> Self
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let root = RootAccount::from_bytes(&crate::util::random_bytes::<_, 32>(rng));
        Simple {
            locked: Some(root),
            unlocked: None,
        }
    }

    #[cfg(all(feature = "rand", feature = "mnemonic"))]
    pub fn generate_with_phrase<R>(rng: &mut R) -> (Self, mnemonic::Mnemonic)
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let phrase = crate::util::gen_phrase(rng, Default::default());

        let seed = Pin::from("").protect::<64>(&phrase.entropy());
        let root = RootAccount::from_bytes(&seed);
        (
            Simple {
                locked: Some(root),
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

        let seed = Pin::from("").protect::<64>(&phrase.entropy());
        Self::from_seed(&seed)
    }

    pub fn from_seed(seed: impl AsRef<[u8]>) -> Self {
        let root = RootAccount::from_bytes(seed.as_ref());
        Simple {
            locked: Some(root),
            unlocked: None,
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
    type Credentials = ();
    type Error = Error;

    async fn unlock<T>(
        &mut self,
        _cred: &Self::Credentials,
        mut cb: impl FnMut(&RootAccount) -> T,
    ) -> Result<T, Self::Error> {
        self.unlocked = self.locked.take();
        let root_account = &self.unlocked.as_ref().unwrap();
        Ok(cb(root_account))
    }
}
