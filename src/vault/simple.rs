use core::future::{ready, Ready};

use crate::{RootAccount, Vault};
use mnemonic::Mnemonic;

/// A vault that holds secrets in memory
pub struct Simple {
    locked: Option<RootAccount>,
    unlocked: Option<RootAccount>,
}

impl Simple {
    /// A vault with a random seed, once dropped the the vault can't be restored
    ///
    /// ```
    /// # use libwallet::{vault, Vault};
    /// # #[async_std::main] async fn main() -> Result<(), ()> {
    /// let (mut vault, _) = vault::Simple::new();
    /// vault.unlock(()).await?;
    /// assert!(vault.get_root().is_some());
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        let root = RootAccount::generate(&mut rand_core::OsRng);
        Simple {
            locked: Some(root),
            unlocked: None,
        }
    }

    #[cfg(feature = "std")]
    pub fn new_with_phrase() -> (Self, Mnemonic) {
        let (root, phrase) =
            RootAccount::generate_with_phrase(&mut rand_core::OsRng, Default::default());
        (
            Simple {
                locked: Some(root),
                unlocked: None,
            },
            phrase,
        )
    }

    // Provide your own seed
    pub fn from_phrase<T: AsRef<str>>(phrase: T) -> Self {
        let phrase = phrase
            .as_ref()
            .parse::<mnemonic::Mnemonic>()
            .expect("mnemonic");
        let root = RootAccount::from_bytes(phrase.entropy());
        Simple {
            locked: Some(root),
            unlocked: None,
        }
    }
}

impl Vault for Simple {
    type Credentials = ();
    type Error = ();
    type AuthDone = Ready<Result<(), Self::Error>>;

    fn unlock(&mut self, _cred: ()) -> Self::AuthDone {
        self.unlocked = self.locked.take();
        ready(Ok(()))
    }

    fn get_root(&self) -> Option<&RootAccount> {
        self.unlocked.as_ref()
    }
}
