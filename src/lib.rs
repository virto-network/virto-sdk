//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.
#![no_std]

#[macro_use]
extern crate alloc;
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use hashbrown::HashMap;

/// Wallet is the main interface to manage and interact with accounts.  
pub struct Wallet {
    seed: String,
    accounts: HashMap<String, Box<dyn Account>>,
}

impl Wallet {
    /// Import a wallet from its mnemonic seed
    /// ```
    /// # use libwallet::Wallet;
    /// let wallet = Wallet::import("test test test test");
    /// ```
    pub fn import(seed: &str) -> Self {
        Wallet {
            seed: seed.to_owned(),
            accounts: HashMap::new(),
        }
    }

    /// Add an account with a given name that is derived
    /// from the master key of the wallet.  
    /// ```
    /// # use libwallet::Wallet;
    /// # let mut wallet = Wallet::import("");
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

