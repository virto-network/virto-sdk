// #![feature(result_option_inspect)]
#![cfg_attr(not(feature = "std"), no_std)]
//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.

mod account;
mod key_pair;
#[cfg(feature = "osvault")]
mod osvault;
#[cfg(feature = "simple")]
mod simple;
#[cfg(feature = "substrate")]
mod substrate_ext;

use serde::{Deserialize, Serialize};

use core::future::Future;
#[cfg(feature = "mnemonic")]
use mnemonic;
use std::collections::HashMap;

pub use account::Account;
pub use key_pair::*;
#[cfg(feature = "mnemonic")]
pub use mnemonic::{Language, Mnemonic};
#[cfg(feature = "osvault")]
pub use osvault::OSVault;
#[cfg(feature = "simple")]
pub use simple::SimpleVault;

/// Abstration for storage of private keys that are protected by a password.
pub trait Vault {
    type Pair: Pair;
    type PairFut: Future<Output = Result<Self::Pair>>;

    fn unlock<C>(&mut self, credentials: C) -> Self::PairFut;
}

pub type Result<T> = core::result::Result<T, Error>;
type SignatureOf<V> = <<V as Vault>::Pair as Pair>::Signature;

/// Wallet is the main interface to manage and interact with accounts.  
#[derive(Debug)]
pub struct Wallet<V>
where
    V: Vault,
{
    vault: V,
    root: Option<Account<V::Pair>>,
    subaccounts: HashMap<String, Account<V::Pair>>,
}

impl<V> From<V> for Wallet<V>
where
    V: Vault,
{
    fn from(vault: V) -> Self {
        Wallet {
            vault,
            root: None,
            subaccounts: HashMap::new(),
        }
    }
}

impl<V> Wallet<V>
where
    V: Vault,
{
    pub fn new(vault: V) -> Self {
        vault.into()
    }

    /// The root account represents the public/private key as it's returned by the vault.
    /// It's recommended to create sub-accoutns and used those instead.
    pub fn root_account(&self) -> Result<&Account<V::Pair>> {
        self.root.as_ref().ok_or(Error::Locked)
    }

    pub fn create_sub_account(
        &mut self,
        name: &str,
        derivation_path: &str,
    ) -> Result<&Account<V::Pair>> {
        let root = self.root_account()?;
        let subaccount = root
            .derive_subaccount(name, derivation_path)
            .ok_or(Error::DeriveError)?;
        self.subaccounts.insert(name.to_string(), subaccount);
        Ok(self.subaccounts.get(name).unwrap())
    }

    /// A locked wallet uses its vault to retrive the key pair used to sign transactions.
    /// ```
    /// # use libwallet::{Wallet, Error, SimpleVault, sr25519};
    /// # use std::convert::TryInto;
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// let vault: SimpleVault<sr25519::Pair> = "//Alice".into();
    /// let mut wallet = Wallet::from(vault);
    /// if wallet.is_locked() {
    ///     wallet = wallet.unlock(()).await?;
    /// }
    /// # assert_eq!(wallet.is_locked(), false);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock<C>(mut self, credentials: C) -> Result<Self> {
        if !self.is_locked() {
            return Ok(self);
        }
        let pair = self.vault.unlock(credentials).await?;
        self.root = Some(Account::from_pair(pair));
        Ok(self)
    }

    pub fn is_locked(&self) -> bool {
        self.root.is_none()
    }

    /// Sign a message with the default account and return the 512bit signature.
    /// ```
    /// # use libwallet::{Wallet, SimpleVault, sr25519, Result};
    /// # #[async_std::main] async fn main() -> Result<()> {
    ///
    /// let wallet = Wallet::new(SimpleVault::<sr25519::Pair>::new()).unlock(()).await?;
    /// let signature = wallet.sign(&[0x01, 0x02, 0x03]);
    /// # assert!(signature.is_ok());
    /// # Ok(()) }
    /// ```
    pub fn sign(&self, message: &[u8]) -> Result<SignatureOf<V>> {
        Ok(self.root_account()?.sign(message))
    }

    /// Save data to be signed later by root account
    /// ```
    /// # use libwallet::{Wallet, SimpleVault, sr25519, Result};
    /// # #[async_std::main] async fn main() -> Result<()> {
    ///
    /// let mut wallet = Wallet::new(SimpleVault::<sr25519::Pair>::new()).unlock(()).await?;
    /// let res = wallet.sign_later(&[0x01, 0x02, 0x03]);
    /// assert!(res.is_ok());
    /// # Ok(()) }
    /// ```
    pub fn sign_later(&mut self, message: &[u8]) -> Result<()> {
        self.root
            .as_mut()
            .map(|a| a.add_to_pending(message))
            .ok_or(Error::Locked)
    }

    /// Try to sign all messages in the queue of an account
    /// Returns signed transactions
    /// ```
    /// # use libwallet::{Wallet, SimpleVault, sr25519, Result};
    /// # #[async_std::main] async fn main() -> Result<()> {
    ///
    /// let mut wallet = Wallet::new(SimpleVault::<sr25519::Pair>::new()).unlock(()).await?;
    /// wallet.sign_later(&[0x01, 0x02, 0x03]);
    /// wallet.sign_later(&[0x01, 0x02]);
    /// wallet.sign_pending("ROOT");
    /// let res = wallet.get_pending("ROOT").collect::<Vec<_>>();
    /// assert!(res.is_empty());
    /// # Ok(()) }
    /// ```
    pub fn sign_pending(&mut self, name: &str) -> Vec<(Vec<u8>, SignatureOf<V>)> {
        match name {
            "ROOT" => self
                .root
                .as_mut()
                .map(|a| a.sign_pending())
                .unwrap_or_default(),
            _ => todo!(), //search sub-accounts
        }
    }

    /// Iteratate over the messages with pending signature of the named account.
    /// It panics if the wallet is locked.
    ///
    /// ```
    /// # use libwallet::{Wallet, SimpleVault, sr25519, Result};
    /// # #[async_std::main] async fn main() -> Result<()> {
    ///
    /// let mut wallet = Wallet::new(SimpleVault::<sr25519::Pair>::new()).unlock(()).await?;
    /// wallet.sign_later(&[0x01, 0x02, 0x03]);
    /// wallet.sign_later(&[0x01, 0x02]);
    /// let res = wallet.get_pending("ROOT").collect::<Vec<_>>();
    /// assert_eq!(vec![vec![0x01, 0x02, 0x03], vec![0x01, 0x02]], res);
    /// # Ok(()) }
    /// ```
    pub fn get_pending(&self, name: &str) -> impl Iterator<Item = &[u8]> {
        match name {
            "ROOT" => self.root_account().unwrap().get_pending(),
            _ => todo!(), //get sub-accounts
        }
    }

    /// Switch the network used by the root account which is used by
    /// default when deriving new sub-accounts
    pub fn switch_default_network(&mut self, net: &str) -> Result<&Account<V::Pair>> {
        let root = self.root.take();
        self.root = root.map(|a| a.switch_network(net));
        self.root_account()
    }
}

#[cfg(all(feature = "simple", feature = "std"))]
impl Default for Wallet<SimpleVault<sr25519::Pair>> {
    fn default() -> Self {
        Wallet {
            vault: SimpleVault::new(),
            root: None,
            subaccounts: HashMap::new(),
        }
    }
}

// Represents the blockchain network in use by an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Network {
    // For substrate based blockchains commonly formatted as SS58
    // that are distinguished by their address prefix. 42 is the generic prefix.
    #[cfg(feature = "substrate")]
    Substrate(u16),
    // Space for future supported networks(e.g. ethereum, bitcoin)
}

impl Default for Network {
    fn default() -> Self {
        #[cfg(feature = "substrate")]
        Network::Substrate(42)
    }
}

impl core::fmt::Display for Network {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::Substrate(p) => write!(f, "{}", p),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Locked,
    InvalidCredentials,
    DeriveError,
    #[cfg(feature = "mnemonic")]
    InvalidPhrase,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::Locked => write!(f, "Locked"),
            Error::InvalidCredentials => write!(f, "Invalid credentials"),
            Error::DeriveError => write!(f, "Cannot derive"),
            Error::InvalidPhrase => write!(f, "Invalid phrase"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

#[cfg(feature = "mnemonic")]
impl From<mnemonic::Error> for Error {
    fn from(_: mnemonic::Error) -> Self {
        Error::InvalidPhrase
    }
}
