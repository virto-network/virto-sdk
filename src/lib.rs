//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.
#![no_std]

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Wallet is the main interface to manage and interact with accounts.  
pub struct Wallet {
    accounts: Vec<Account>,
}

impl Wallet {
    /// Create new wallet
    pub fn new() -> Self {
        Self {
            accounts: Vec::new(),
        }
    }

    /// import an account from a seed
    /// ```
    /// # use libwallet::Wallet;
    /// # let mut wallet = Wallet::new();
    /// wallet.import_account("test test test test");
    /// ```
    pub fn import_account(&mut self, seed: &str) {
        self.accounts.push(Account {
            seed: seed.to_string(),
            pk: Vec::new(),
        });
    }
}

pub struct Account {
    pk: Vec<u8>,
    seed: String,
}
