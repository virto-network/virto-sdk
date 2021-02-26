#![cfg_attr(not(feature = "std"), no_std)]
//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.

#[cfg(any(feature = "std", test))]
#[macro_use]
extern crate std;
#[cfg(all(not(feature = "std"), not(test)))]
#[macro_use]
extern crate core as std;

extern crate alloc;
use alloc::{boxed::Box, format};

use async_trait::async_trait;
use core::str::FromStr;
use sp_core::hexdisplay::HexDisplay;
pub use sp_core::{
    crypto::{CryptoType, Dummy as DummyPair},
    ecdsa, ed25519, sr25519, Pair,
};

#[async_trait(?Send)]
pub trait Vault: CryptoType {
    async fn unlock(&self, password: &str) -> Result<Self::Pair>;
}

/// A vault that holds secrets in memory
pub struct SimpleVault<T: Pair> {
    seed: T::Seed,
}

impl<T: Pair> CryptoType for SimpleVault<T> {
    type Pair = T;
}

impl<T: Pair> SimpleVault<T> {
    /// A vault with a random seed, once dropped the the vault can't be restored
    /// ```
    /// # use libwallet::{SimpleVault, Vault, Result, DummyPair};
    /// # #[async_std::main] async fn main() -> Result<()> {
    /// let vault = SimpleVault::<DummyPair>::new();
    /// assert!(vault.unlock("").await.is_ok());
    /// # Ok(()) }
    /// ```
    pub fn new() -> Self {
        SimpleVault {
            seed: <Self as CryptoType>::Pair::generate().1,
        }
    }

    /// A vault with a password and random seed
    pub fn new_with_password(pwd: &str) -> Self {
        SimpleVault {
            seed: <Self as CryptoType>::Pair::generate_with_phrase(Some(pwd)).2,
        }
    }
}

impl<T: Pair> From<&str> for SimpleVault<T> {
    fn from(s: &str) -> Self {
        s.parse().expect("valid secret string")
    }
}

impl<T: Pair> FromStr for SimpleVault<T> {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let seed = <Self as CryptoType>::Pair::from_string_with_seed(s, None)
            .map_err(|_| Error::InvalidPhrase)?
            .1
            .ok_or(Error::InvalidPhrase)?;
        Ok(SimpleVault { seed })
    }
}

#[async_trait(?Send)]
impl<T: Pair> Vault for SimpleVault<T> {
    async fn unlock(&self, pwd: &str) -> Result<T> {
        let phrase = format!("0x{}", HexDisplay::from(&self.seed.as_ref()));
        Ok(
            <Self as CryptoType>::Pair::from_string_with_seed(&phrase, Some(pwd))
                .map_err(|_| Error::InvalidPhrase)?
                .0,
        )
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Wallet is the main interface to manage and interact with accounts.  
#[derive(Debug)]
pub struct Wallet<V: Vault> {
    vault: V,
    pair: Option<V::Pair>,
}

impl<V: Vault> From<V> for Wallet<V> {
    fn from(vault: V) -> Self {
        Wallet { vault, pair: None }
    }
}

pub type Account<T> = <T as CryptoType>::Pair;

impl<V: Vault> Wallet<V> {
    /// Wallets have a root account that is used by default to sign messages.
    /// Other sub-accounts can be created from this main account.
    pub fn root_account(&self) -> Result<&Account<V>> {
        self.pair.as_ref().ok_or(Error::Locked)
    }

    /// A locked wallet can use a vault to retrive its secret seed.
    /// ```
    /// # use libwallet::{Wallet, Error, SimpleVault, sr25519};
    /// # use std::convert::TryInto;
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// let vault: SimpleVault<sr25519::Pair> = "//Alice".into();
    /// let mut wallet = Wallet::from(vault);
    /// if wallet.is_locked() {
    ///     wallet.unlock("").await?;
    /// }
    /// # assert_eq!(wallet.is_locked(), false);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock(&mut self, password: &str) -> Result<()> {
        if !self.is_locked() {
            return Ok(());
        }
        self.pair = Some(self.vault.unlock(password).await?);
        Ok(())
    }

    pub fn is_locked(&self) -> bool {
        self.pair.is_none()
    }

    /// Sign a message with the default account and return the 512bit signature.
    /// ```
    /// # use libwallet::{Wallet, SimpleVault, sr25519, Result};
    /// # #[async_std::main] async fn main() -> Result<()> {
    ///
    /// let mut wallet: Wallet<_> = SimpleVault::<sr25519::Pair>::new().into();
    /// wallet.unlock("").await;
    /// let signature = wallet.sign(&[0x01, 0x02, 0x03]);
    /// assert!(signature.is_ok());
    /// # Ok(()) }
    /// ```
    pub fn sign(&self, msg: &[u8]) -> Result<<Account<V> as Pair>::Signature> {
        Ok(self.root_account()?.sign(msg))
    }
}

impl<P: Pair> Default for Wallet<SimpleVault<P>> {
    fn default() -> Self {
        Wallet {
            vault: SimpleVault::<P>::new(),
            pair: None,
        }
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum Error {
    #[cfg_attr(feature = "std", error("Invalid mnemonic phrase"))]
    InvalidPhrase,
    #[cfg_attr(feature = "std", error("Invalid password"))]
    InvalidPassword,
    #[cfg_attr(feature = "std", error("Wallet is locked"))]
    Locked,
}
