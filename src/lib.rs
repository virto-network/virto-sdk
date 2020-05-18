//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(any(feature = "std", test))]
#[macro_use]
extern crate std;

#[cfg(all(not(feature = "std"), not(test)))]
#[macro_use]
extern crate core as std;

use crate::std::fmt;

#[macro_use]
extern crate alloc;
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use hashbrown::HashMap;

use bip39::Seed;
pub use bip39::{Language, Mnemonic, MnemonicType};

#[cfg(feature = "chain")]
pub mod chain;

/// Wallet is the main interface to manage and interact with accounts.  
pub struct Wallet {
    seed: Seed,
    accounts: HashMap<String, Box<dyn Account>>,
}

impl Wallet {
    /// Import a wallet from its mnemonic seed
    /// ```
    /// # use libwallet::{Language, Wallet, mnemonic};
    /// let phrase = mnemonic(Language::English);
    /// let mut wallet = Wallet::import(&phrase).unwrap();
    /// ```
    pub fn import(seed_phrase: &str) -> Result<Self, Error> {
        let mnemonic = Mnemonic::from_phrase(seed_phrase, Language::English)
            .map_err(|_| Error::InvalidPhrase)?;
        let seed = Seed::new(&mnemonic, "");
        Ok(Wallet {
            seed,
            accounts: HashMap::new(),
        })
    }

    /// Add an account with a given name that is derived
    /// from the master key of the wallet.  
    /// ```
    /// # use libwallet::{Language, Wallet, mnemonic};
    /// # let phrase = mnemonic(Language::English);
    /// # let mut wallet = Wallet::import(&phrase).unwrap();
    /// let account = wallet.add_account("moneyyy");
    /// ```
    pub fn add_account(&mut self, name: &str) -> &Box<dyn Account> {
        let account = self.derive(name);
        self.accounts.insert(name.to_owned(), Box::new(account));
        &self.accounts.get(name).unwrap()
    }

    fn derive(&self, _phrase: &str) -> impl Account {}
}

pub trait Account {
    fn address(&self) -> Vec<u8>;
    fn pk(&self) -> Vec<u8>;
}

impl Account for () {
    fn address(&self) -> Vec<u8> {
        vec![0]
    }
    fn pk(&self) -> Vec<u8> {
        vec![0]
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidPhrase,
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidPhrase => write!(f, "Invalid mnemonic phrase"),
        }
    }
}

/// Generate a 24 word mnemonic phrase with words in the specified language.
/// ```
/// # use libwallet::{mnemonic, Language};
/// let phrase = mnemonic(Language::English);
/// # let words = phrase.split_whitespace().count();
/// # assert_eq!(words, 24);
/// ```
pub fn mnemonic(lang: Language) -> String {
    Mnemonic::new(MnemonicType::Words24, lang)
        .phrase()
        .to_owned()
}
