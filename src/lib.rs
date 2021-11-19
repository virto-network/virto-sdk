#![cfg_attr(not(feature = "std"), no_std)]
//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.
extern crate alloc;
use alloc::boxed::Box;
#[cfg(feature = "simple")]
mod simple;

pub use async_trait::async_trait;
use core::convert::{TryFrom, TryInto};
#[cfg(feature = "simple")]
pub use simple::SimpleVault;
use sp_core::crypto::Ss58AddressFormat;
pub use sp_core::{
    crypto::{CryptoType, Pair},
    ecdsa, ed25519,
    hexdisplay::HexDisplay,
    sr25519,
};

/// Abstration for storage of private keys that are protected by a password.
#[async_trait(?Send)]
pub trait Vault: CryptoType {
    async fn unlock(&self, password: &str) -> Result<Self::Pair>;
}

pub type Result<T> = core::result::Result<T, Error>;

// Represents the blockchain network that this wallet belongs to
#[derive(Debug, Clone)]
pub enum Network {
    // For substrate based blockchains commonly formatted as SS58
    // that are distinguished by their address prefix. 42 is the generic prefix.
    Substrate(u16),
    // Space for future supported networks(e.g. ethereum, bitcoin)
}

impl Default for Network {
    fn default() -> Self {
        Network::Substrate(42)
    }
}

impl From<&Network> for Ss58AddressFormat {
    fn from(n: &Network) -> Self {
        match n {
            Network::Substrate(prefix) => (*prefix).try_into().expect("valid substrate prefix"),
        }
    }
}

impl core::str::FromStr for Network {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Ss58AddressFormat::try_from(s)
            .map(|x| Network::Substrate(x.into()))
            .map_err(|_| Error::InvalidNetwork)
    }
}

#[cfg(feature = "std")]
impl core::fmt::Display for Network {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}", Ss58AddressFormat::from(self))
    }
}

/// Wallet is the main interface to manage and interact with accounts.  
#[derive(Debug)]
pub struct Wallet<V: Vault> {
    vault: V,
    pair: Option<V::Pair>,
    network: Network,
}

impl<V: Vault> From<V> for Wallet<V> {
    fn from(vault: V) -> Self {
        Wallet {
            vault,
            pair: None,
            network: Default::default(),
        }
    }
}

pub type Account<T> = <T as CryptoType>::Pair;

impl<V: Vault> Wallet<V> {
    pub fn new(vault: V, network: Network) -> Self {
        Wallet {
            vault,
            pair: None,
            network,
        }
    }

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

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn switch_network(self, net: impl Into<Network>) -> Self {
        Wallet {
            network: net.into(),
            ..self
        }
    }
}

#[cfg(feature = "std")]
impl<V> core::fmt::Display for Wallet<V>
where
    V: Vault,
    <<V as CryptoType>::Pair as Pair>::Public: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let account = self.root_account().expect("unlocked").public();
        write!(
            f,
            "{}",
            sp_core::crypto::Ss58Codec::to_ss58check_with_version(&account, self.network().into())
        )
    }
}

#[cfg(all(feature = "simple", feature = "std"))]
impl<P: Pair> Default for Wallet<SimpleVault<P>> {
    fn default() -> Self {
        Wallet {
            vault: SimpleVault::<P>::new(),
            pair: None,
            network: Default::default(),
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
    #[cfg_attr(feature = "std", error("Invalid network identifier"))]
    InvalidNetwork,
    #[cfg_attr(feature = "std", error("Wallet is locked"))]
    Locked,
}
