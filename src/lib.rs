#![cfg_attr(not(feature = "std"), no_std)]
//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.
extern crate alloc;
use alloc::boxed::Box;
mod account;
#[cfg(feature = "simple")]
mod simple;
#[cfg(feature = "substrate")]
mod substrate_ext;

use core::convert::TryFrom;

pub use account::Account;
pub use async_trait::async_trait;
#[cfg(feature = "simple")]
pub use simple::SimpleVault;
pub use sp_core::{
    crypto::{CryptoType, Pair},
    ecdsa, ed25519,
    hexdisplay::HexDisplay,
    sr25519,
};

/// Abstration for storage of private keys that are protected by a password.
#[async_trait(?Send)]
pub trait Vault {
    type Pair: Pair;

    async fn unlock(&self, password: &str) -> Result<Self::Pair>;
}

pub type Result<T> = core::result::Result<T, Error>;
type SignatureOf<V> = <<V as Vault>::Pair as Pair>::Signature;

/// Wallet is the main interface to manage and interact with accounts.  
#[derive(Debug)]
pub struct Wallet<V: Vault> {
    vault: V,
    root: Option<Account<'static, V::Pair>>,
}

impl<V: Vault> From<V> for Wallet<V> {
    fn from(vault: V) -> Self {
        Wallet { vault, root: None }
    }
}

impl<V: Vault> Wallet<V> {
    pub fn new(vault: V) -> Self {
        vault.into()
    }

    /// The root account represents the public/private key as it's returned by the vault.
    /// It's recommended to create sub-accoutns and used those instead.
    pub fn root_account(&self) -> Result<&Account<V::Pair>> {
        self.root.as_ref().ok_or(Error::Locked)
    }

    /// A locked wallet uses its vault to retrive the key pair used to sign transactions.
    /// ```
    /// # use libwallet::{Wallet, Error, SimpleVault, sr25519};
    /// # use std::convert::TryInto;
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// let vault: SimpleVault<sr25519::Pair> = "//Alice".into();
    /// let mut wallet = Wallet::from(vault);
    /// if wallet.is_locked() {
    ///     wallet = wallet.unlock("").await?;
    /// }
    /// # assert_eq!(wallet.is_locked(), false);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock(mut self, password: &str) -> Result<Self> {
        if !self.is_locked() {
            return Ok(self);
        }
        let pair = self.vault.unlock(password).await?;
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
    /// let wallet = Wallet::new(SimpleVault::<sr25519::Pair>::new()).unlock("").await?;
    /// let signature = wallet.sign(&[0x01, 0x02, 0x03]);
    /// assert!(signature.is_ok());
    /// # Ok(()) }
    /// ```
    pub fn sign(&self, message: &[u8]) -> Result<SignatureOf<V>> {
        Ok(self.root_account()?.sign(message))
    }

    /// Switch the network used by the root account which is used by
    /// default when deriving new sub-accounts
    pub fn switch_default_network(&mut self, net: &str) -> Result<&Account<V::Pair>> {
        let root = self.root.take();
        let network = net.parse().map_err(|_| Error::InvalidNetwork)?;
        self.root = root.map(|a| a.switch_network(network));
        self.root_account()
    }
}

#[cfg(all(feature = "simple", feature = "std"))]
impl<P: Pair> Default for Wallet<SimpleVault<P>> {
    fn default() -> Self {
        Wallet {
            vault: SimpleVault::<P>::new(),
            root: None,
        }
    }
}

// Represents the blockchain network in use by an account
#[derive(Debug, Clone)]
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

impl core::str::FromStr for Network {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        #[cfg(feature = "substrate")]
        substrate_ext::Ss58AddressFormat::try_from(s)
            .map(|x| Network::Substrate(x.into()))
            .map_err(|_| Error::InvalidNetwork)
    }
}

#[cfg(feature = "std")]
impl core::fmt::Display for Network {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        #[cfg(feature = "substrate")]
        write!(f, "{}", substrate_ext::Ss58AddressFormat::from(self))
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum Error {
    #[cfg_attr(feature = "std", error("Invalid mnemonic phrase"))]
    InvalidPhrase,
    #[cfg_attr(feature = "std", error("Invalid password"))]
    InvalidPassword,
    #[cfg_attr(feature = "std", error("Invalid network identifier"))]
    InvalidNetwork,
    #[cfg_attr(feature = "std", error("Wallet is locked"))]
    Locked,
}
