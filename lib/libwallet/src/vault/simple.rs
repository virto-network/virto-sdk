use crate::util::{seed_from_entropy, Pin};
use crate::{
    vault::utils::{AccountSigner, RootAccount},
    Vault,
};
use core::marker::PhantomData;

/// A vault that holds secrets in memory
pub struct Simple<S, const N: usize = 32> {
    locked: Option<[u8; N]>,
    unlocked: Option<[u8; N]>,
    _phantom: PhantomData<S>,
}

impl<S, const N: usize> Simple<S, N> {
    /// A vault with a random seed, once dropped the the vault can't be restored
    ///
    /// ```
    /// # use libwallet::{vault, Error, Derive, Pair, Vault};
    /// # type SimpleVault = vault::Simple<String>;
    /// # type Result = std::result::Result<(), <SimpleVault as Vault>::Error>;
    /// # #[async_std::main] async fn main() -> Result {
    /// let mut vault = SimpleVault::generate(&mut rand_core::OsRng);
    /// let root = vault.unlock(None, None).await?;
    /// # Ok(())
    /// }
    /// ```
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
    // Provide your own seed
    pub fn from_phrase(phrase: impl AsRef<str>) -> Self {
        use core::convert::TryInto;
        mnemonic::Mnemonic::validate(phrase.as_ref()).expect("its a valid mnemonic");
        // Count the number of words in the phrase
        let mnemonic = mnemonic::Mnemonic::from_phrase(phrase.as_ref()).expect("its a valid mnemonic");

        let raw_entropy = mnemonic.entropy();

        Simple {
            locked: Some(raw_entropy.try_into().expect("its a valid entropy")),
            unlocked: None,
            _phantom: Default::default(),
        }
    }

    fn get_key(&self, pin: Pin) -> Result<RootAccount, Error> {
        if let Some(entropy) = self.unlocked {
            let seed = &entropy;
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

impl<S: AsRef<str>, const N: usize> Vault for Simple<S, N> {
    type Credentials = Option<Pin>;
    type Error = Error;
    type Id = Option<S>;
    type Account = AccountSigner;

    async fn unlock(
        &mut self,
        path: Self::Id,
        creds: impl Into<Self::Credentials>,
    ) -> Result<Self::Account, Self::Error> {
        self.unlocked = self.locked.take();
        let pin = creds.into();
        let root_account = self.get_key(pin.unwrap_or_default())?;
        let path = path.as_ref().map(|x| x.as_ref());
        Ok(AccountSigner::new(path).unlock(&root_account))
    }
}
