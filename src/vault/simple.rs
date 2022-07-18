use core::future::{ready, Ready};

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
    /// # use libwallet::{vault, Vault, Error};
    /// # type Result = std::result::Result<(), <vault::Simple as Vault>::Error>;
    /// # #[async_std::main] async fn main() -> Result {
    /// let mut vault = vault::Simple::new();
    /// vault.unlock(()).await?;
    /// assert!(vault.get_root().is_some());
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        let root =
            RootAccount::from_bytes(&crate::util::random_bytes::<_, 32>(&mut rand_core::OsRng));
        Simple {
            locked: Some(root),
            unlocked: None,
        }
    }

    #[cfg(feature = "std")]
    pub fn new_with_phrase() -> (Self, mnemonic::Mnemonic) {
        let phrase = crate::util::gen_phrase(&mut rand_core::OsRng, Default::default());
        let root = RootAccount::from_bytes(phrase.entropy());
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
        Self::from_seed(phrase.entropy())
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
    type AuthDone = Ready<Result<(), Self::Error>>;

    fn unlock(&mut self, _cred: impl Into<Self::Credentials>) -> Self::AuthDone {
        self.unlocked = self.locked.take();
        ready(Ok(()))
    }

    fn get_root(&self) -> Option<&RootAccount> {
        self.unlocked.as_ref()
    }
}
