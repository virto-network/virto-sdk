use core::future::{ready, Ready};

use crate::{RootAccount, Vault};
use mnemonic::Mnemonic;

/// A vault that holds secrets in memory
pub struct SimpleVault {
    root: RootAccount,
}

impl SimpleVault {
    /// A vault with a random seed, once dropped the the vault can't be restored
    /// ```
    /// # use libwallet::{SimpleVault, Vault, Result, sr25519};
    /// # #[async_std::main] async fn main() -> Result<()> {
    /// let mut vault = SimpleVault::<sr25519::Pair>::new();
    /// assert!(vault.unlock(()).await.is_ok());
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "std")]
    pub fn new() -> (Self, Mnemonic) {
        let (root, phrase) =
            RootAccount::generate_with_phrase(&mut rand_core::OsRng, Default::default());
        (SimpleVault { root }, phrase)
    }

    // Provide your own seed
    pub fn from_phrase<T: AsRef<str>>(phrase: T) -> Self {
        let phrase = phrase
            .as_ref()
            .parse::<mnemonic::Mnemonic>()
            .expect("mnemonic");
        SimpleVault {
            root: RootAccount::from_bytes(phrase.entropy()),
        }
    }
}

impl Vault for SimpleVault {
    type Credentials = ();
    type Error = ();
    type AuthDone = Ready<Result<(), Self::Error>>;

    fn unlock(&mut self, _cred: ()) -> Self::AuthDone {
        ready(Ok(()))
    }

    fn get_root(&self) -> Option<&RootAccount> {
        Some(&self.root)
    }
}
