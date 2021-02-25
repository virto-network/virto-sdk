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
use alloc::{boxed::Box, string::String, string::ToString};

use async_trait::async_trait;
pub use bip39::{Language, Mnemonic, MnemonicType, Seed};
use sp_core::{sr25519, Pair};
use std::fmt::{self, Display};
use std::ops::Deref;

#[async_trait]
pub trait Vault: fmt::Debug + Send {
    async fn unlock(&self, password: &str) -> Result<Seed>;
}

#[async_trait]
impl Vault for () {
    async fn unlock(&self, _: &str) -> Result<Seed> {
        Err(Error::NoVault)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Wallet is the main interface to manage and interact with accounts.  
#[derive(Debug)]
pub struct Wallet<V: Vault> {
    seed: Option<Seed>,
    vault: Option<V>,
}

impl Wallet<()> {
    pub fn new() -> Self {
        Default::default()
    }

    /// Import a wallet from its mnemonic seed
    /// ```
    /// # use libwallet::{Language, Wallet, mnemonic};
    /// let m = mnemonic(Language::English);
    /// let mut wallet = Wallet::import(m.phrase(), "foo").unwrap();
    /// # assert_eq!(wallet.is_locked(), false);
    /// ```
    pub fn import(seed_phrase: &str, password: &str) -> Result<Self> {
        let mnemonic = Mnemonic::from_phrase(&seed_phrase, Language::English)
            .map_err(|_| Error::InvalidPhrase)?;
        let seed = bip39::Seed::new(&mnemonic, &password).into();
        Ok(Wallet {
            seed: Some(seed),
            ..Default::default()
        })
    }

    /// Generate a new wallet with a 24 word english mnemonic seed
    pub fn generate(password: &str) -> (Self, String) {
        let m = mnemonic(Language::English);
        (Wallet::import(m.phrase(), password).unwrap(), m.into())
    }
}

impl<V: Vault> Wallet<V> {
    /// In case the wallet was not imported directly from a mnemonic phrase
    /// it needs a vault to unlock the stored seed. This method creates a new
    /// wallet copying data from the current wallet(none at the moment).
    pub fn with_vault<V2: Vault>(self, vault: V2) -> Wallet<V2> {
        Wallet {
            vault: Some(vault),
            seed: None,
        }
    }

    /// Wallets have a root account that is used by default to sign messages.
    /// Other sub-accounts can be created from this main account.
    pub fn root_account(&self) -> Result<Account<sr25519::Pair>> {
        let seed = self.seed.as_ref().ok_or(Error::Locked)?.as_ref();
        let root = Account::from_seed(seed);
        Ok(root)
    }

    /// A locked wallet can use a vault to retrive its secret seed.
    /// ```
    /// # use libwallet::{Wallet, Error, Seed, mnemonic, Language, Vault};
    /// # use std::convert::TryInto;
    /// # #[derive(Debug, Default)] struct Dummy;
    /// # #[async_trait::async_trait] impl Vault for Dummy {
    /// #   async fn unlock(&self, pwd: &str) -> Result<Seed, Error> {
    /// #       Ok(Seed::new(&mnemonic(Language::English), pwd))
    /// #   }
    /// # }
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let dummy_vault = Dummy{};
    /// let mut wallet = Wallet::new().with_vault(dummy_vault);
    /// if wallet.is_locked() {
    ///     wallet.unlock("some password").await?;
    /// }
    /// # assert_eq!(wallet.is_locked(), false);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock(&mut self, password: &str) -> Result<()> {
        if !self.is_locked() {
            return Ok(());
        }
        let seed = self
            .vault
            .as_ref()
            .ok_or(Error::NoVault)?
            .unlock(password)
            .await?;
        self.seed = Some(seed).into();
        Ok(())
    }

    pub fn is_locked(&self) -> bool {
        self.seed.is_none()
    }

    /// Sign a message with the default account and return the 512bit signature.
    /// ```
    /// # use libwallet::Wallet;
    /// let (wallet, _) = Wallet::generate("foo");
    /// let signature = wallet.sign(&[0x01, 0x02, 0x03]);
    /// assert!(signature.is_ok());
    /// ```
    pub fn sign(&self, msg: &[u8]) -> Result<[u8; 64]> {
        let signature = self.root_account()?.sign(msg);
        Ok(signature.into())
    }
}

impl<V: Vault> Default for Wallet<V> {
    fn default() -> Self {
        Wallet {
            seed: None,
            vault: None,
        }
    }
}

/// A derived accout from the wallet's root key pair
pub struct Account<P>
where
    P: Pair,
    P::Public: Display,
{
    pair: P,
}

impl<P> Account<P>
where
    P: Pair,
    P::Public: Display,
{
    pub fn from_seed(seed: &[u8]) -> Self {
        let pair = P::from_seed_slice(&seed[..32]).expect("seed is valid");
        Account { pair }
    }

    /// Unique identifier of the account where funds can be sent to.
    /// Often is the encoded hash of the public key.
    pub fn id(&self) -> String {
        self.pair.public().to_string()
    }
}

impl<P> From<&str> for Account<P>
where
    P: Pair,
    P::Public: Display,
{
    fn from(s: &str) -> Self {
        Account {
            pair: P::from_string(s, None).unwrap(),
        }
    }
}

impl<P> Deref for Account<P>
where
    P: Pair,
    P::Public: Display,
{
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.pair
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum Error {
    #[cfg_attr(feature = "std", error("Invalid mnemonic phrase"))]
    InvalidPhrase,
    #[cfg_attr(feature = "std", error("Invalid password"))]
    InvalidPasword,
    #[cfg_attr(feature = "std", error("Wallet is locked"))]
    Locked,
    #[cfg_attr(feature = "std", error("Can't unlock, no vault was configured"))]
    NoVault,
}

/// A new 24 word mnemonic phrase.
/// ```
/// # use libwallet::{mnemonic, Language};
/// let m = mnemonic(Language::English);
/// # let words = m.phrase().split_whitespace().count();
/// # assert_eq!(words, 24);
/// ```
pub fn mnemonic(lang: Language) -> Mnemonic {
    Mnemonic::new(MnemonicType::Words24, lang)
}
