use core::convert::TryInto;

use arrayvec::ArrayVec;

use crate::util::{seed_from_entropy, Pin};
use crate::{
    vault::utils::{AccountSigner, RootAccount},
    Derive, Vault,
};

/// A vault that holds secrets in memory
pub struct Simple {
    locked: Option<[u8; 32]>,
    unlocked: Option<[u8; 32]>,
}

impl Simple {
    /// A vault with a random seed, once dropped the the vault can't be restored
    ///
    /// ```
    /// # use libwallet::{vault, Error, Derive, Pair, Vault};
    /// # type Result = std::result::Result<(), <vault::Simple as Vault>::Error>;
    /// # #[async_std::main] async fn main() -> Result {
    /// let mut vault = vault::Simple::generate(&mut rand_core::OsRng);
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
            locked: Some(crate::util::random_bytes::<_, 32>(rng)),
            unlocked: None,
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
        let phrase = phrase
            .as_ref()
            .parse::<mnemonic::Mnemonic>()
            .expect("mnemonic");
        let entropy = phrase
            .entropy()
            .try_into()
            .expect("Size should be 32 bytes");

        Simple {
            locked: Some(entropy),
            unlocked: None,
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

impl Vault for Simple {
    type Credentials = Option<Pin>;
    type Error = Error;
    type Id = Option<ArrayVec<u8, 10>>;
    type Account = AccountSigner;

    async fn unlock(
        &mut self,
        path: Self::Id,
        creds: impl Into<Self::Credentials>,
    ) -> Result<Self::Account, Self::Error> {
        self.unlocked = self.locked.take();
        let pin = creds.into();
        let root_account = self.get_key(pin.unwrap_or_default())?;
        let path = path.as_ref().map(|r| {
            core::str::from_utf8(r.as_slice())
                .expect("it must be a valid utf8 string")
        });

        Ok(AccountSigner::new(path).unlock(&root_account))
    }
}
