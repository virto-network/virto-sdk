// #![feature(result_option_inspect)]
#![cfg_attr(not(feature = "std"), no_std)]
//! `libwallet` is the one-stop tool to build easy, slightly opinionated crypto wallets
//! that run in all kinds of environments and plattforms including embedded hardware,
//! mobile apps or the Web.
//! It's easy to extend implementing different vault backends and it's designed to
//! be compatible with all kinds of key formats found in many different blockchains.
#[cfg(not(any(feature = "sr25519")))]
compile_error!("Enable at least one type of signature algorithm");

mod account;
mod key_pair;
#[cfg(feature = "substrate")]
mod substrate_ext;

use arrayvec::ArrayVec;
use core::{convert::TryInto, future::Future};
use key_pair::any::AnySignature;

#[cfg(feature = "mnemonic")]
use mnemonic;

pub use account::Account;
pub use key_pair::*;
#[cfg(feature = "mnemonic")]
pub use mnemonic::{Language, Mnemonic};
pub mod vault;

const MSG_MAX_SIZE: usize = u8::MAX as usize;
type Message = ArrayVec<u8, { MSG_MAX_SIZE }>;

/// Abstration for storage of private keys that are protected by some credentials.
pub trait Vault {
    type Credentials;
    type AuthDone: Future<Output = core::result::Result<(), Self::Error>>;
    type Error;

    /// Use a set of credentials to make the guarded keys available to the user.
    /// It returns a `Future` to allow for vaults that might take an arbitrary amount
    /// of time getting the secret ready like waiting for some user phisical interaction.
    fn unlock(&mut self, cred: Self::Credentials) -> Self::AuthDone;

    /// Get the root account container of the supported private key pairs
    /// if the vault hasn't been unlocked it should return `None`
    fn get_root(&self) -> Option<&RootAccount>;
}

/// The root account is a container of the key pairs stored in the vault and cannot be
/// used to sign messages directly, we always derive new key pairs from it to create
/// and use accounts with the wallet.
#[derive(Debug)]
pub struct RootAccount {
    #[cfg(feature = "substrate")]
    sub: key_pair::sr25519::Pair,
}

impl RootAccount {
    fn from_bytes(seed: &[u8]) -> Self {
        RootAccount {
            #[cfg(feature = "substrate")]
            sub: <key_pair::sr25519::Pair as crate::Pair>::from_bytes(seed),
        }
    }

    #[cfg(feature = "rand")]
    fn generate<R>(rng: &mut R) -> Self
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let seed = util::random_bytes::<_, 32>(rng);
        Self::from_bytes(&seed)
    }

    #[cfg(feature = "rand")]
    fn generate_with_phrase<R>(rng: &mut R, lang: Language) -> (Self, Mnemonic)
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let seed = util::random_bytes::<_, 32>(rng);
        let phrase = mnemonic::Mnemonic::from_entropy_in(lang, seed.as_ref()).expect("seed valid");
        (Self::from_bytes(&seed), phrase)
    }
}

impl<'a> Derive for &'a RootAccount {
    type Pair = any::Pair;

    fn derive(&self, path: &str) -> Self::Pair
    where
        Self: Sized,
    {
        match &path[..2] {
            "//" => self.sub.derive(path).into(),
            "m/" => unimplemented!(),
            _ => self.sub.derive("//default").into(),
        }
    }
}

/// Wallet is the main interface to interact with the accounts of a user.
///
/// Before being able to sign messages a wallet must be unlocked using valid credentials
/// supported by the underlying vault.
///
/// Wallets can hold many user defined accounts and always have one account set as "default",
/// if no account is set as default one is generated and will be used to sign messages when no account is specified.
///
/// Wallets also support queuing and bulk signing of messages in case transactions need to be reviewed before signing.
#[derive(Debug)]
pub struct Wallet<V, const A: usize = 5, const M: usize = A> {
    vault: V,
    default_account: Account,
    accounts: ArrayVec<Account, A>,
    pending_sign: ArrayVec<(Message, Option<u8>), M>, // message -> account index or default
}

impl<V, const A: usize, const M: usize> Wallet<V, A, M>
where
    V: Vault,
{
    /// Create a new Wallet with a default account
    pub fn new(vault: V) -> Self {
        Wallet {
            vault,
            default_account: Account::new(None),
            accounts: ArrayVec::new_const(),
            pending_sign: ArrayVec::new(),
        }
    }

    /// Get the account currently set as default
    pub fn default_account(&self) -> &Account {
        &self.default_account
    }

    /// Use credentials to unlock the vault.
    ///
    /// ```
    /// # use libwallet::{Wallet, Error, vault};
    /// # use std::convert::TryInto;
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let (vault, _) = vault::Simple::new();
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// if wallet.is_locked() {
    ///     wallet.unlock(()).await?;
    /// }
    ///
    /// assert!(!wallet.is_locked());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock(&mut self, credentials: V::Credentials) -> Result<(), Error> {
        if self.is_locked() {
            self.vault
                .unlock(credentials)
                .await
                .map_err(|_| Error::InvalidCredentials)?;
            self.default_account.unlock(self.vault.get_root().unwrap());
        }
        Ok(())
    }

    /// Check if the vault has been unlocked.
    pub fn is_locked(&self) -> bool {
        self.vault.get_root().is_none()
    }

    /// Sign a message with the default account and return the signature.
    /// The wallet needs to be unlocked.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error, Signer};
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let (vault, _) = vault::Simple::new();
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.unlock(()).await?;
    ///
    /// let msg = &[0x12, 0x34, 0x56];
    /// let signature = wallet.sign(msg);
    ///
    /// assert!(wallet.default_account().verify(msg, signature.as_ref()));
    /// # Ok(()) }
    /// ```
    pub fn sign(&self, message: &[u8]) -> impl Signature {
        assert!(!self.is_locked());
        self.default_account().sign_msg(message)
    }

    /// Save data to be signed some time later.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error};
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let (vault, _) = vault::Simple::new();
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.sign_later(&[0x01, 0x02, 0x03]);
    ///
    /// assert_eq!(wallet.pending().count(), 1);
    /// # Ok(()) }
    /// ```
    pub fn sign_later<T>(&mut self, message: T)
    where
        T: AsRef<[u8]>,
    {
        let msg = message.as_ref();
        let msg = msg
            .try_into()
            .unwrap_or_else(|_| msg[..MSG_MAX_SIZE].try_into().unwrap());
        self.pending_sign.push((msg, None));
    }

    /// Try to sign all messages in the queue returning the list of signatures
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error};
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let (vault, _) = vault::Simple::new();
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.unlock(()).await?;
    ///
    /// wallet.sign_later(&[0x01, 0x02, 0x03]);
    /// wallet.sign_later(&[0x04, 0x05, 0x06]);
    /// let signatures = wallet.sign_pending();
    ///
    /// assert_eq!(signatures.len(), 2);
    /// assert_eq!(wallet.pending().count(), 0);
    /// # Ok(()) }
    /// ```
    pub fn sign_pending(&mut self) -> ArrayVec<AnySignature, M> {
        let mut signatures = ArrayVec::new();
        for (msg, a) in self.pending_sign.take() {
            let account = a
                .map(|idx| self.account(idx))
                .unwrap_or_else(|| self.default_account());
            signatures.push(account.sign_msg(&msg));
        }
        signatures
    }

    /// Iteratate over the messages pending for signature for all the accounts.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error};
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let (vault, _) = vault::Simple::new();
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.sign_later(&[0x01]);
    /// wallet.sign_later(&[0x02]);
    /// wallet.sign_later(&[0x03]);
    ///
    /// assert_eq!(wallet.pending().count(), 3);
    /// # Ok(()) }
    /// ```
    pub fn pending(&self) -> impl Iterator<Item = &[u8]> {
        self.pending_sign.iter().map(|(msg, _)| msg.as_ref())
    }

    #[cfg(feature = "subaccounts")]
    pub fn new_account(&mut self, name: &str, derivation_path: &str) -> Result<&Account<V::Pair>> {
        let root = self.root_account()?;
        let subaccount = root
            .derive_subaccount(name, derivation_path)
            .ok_or(Error::DeriveError)?;
        self.subaccounts.insert(name.to_string(), subaccount);
        Ok(self.subaccounts.get(name).unwrap())
    }

    fn account(&self, idx: u8) -> &Account {
        &self.accounts[idx as usize]
    }
}

/// Represents the blockchain network in use by an account
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Network {
    // For substrate based blockchains commonly formatted as SS58
    // that are distinguished by their address prefix. 42 is the generic prefix.
    #[cfg(feature = "substrate")]
    Substrate(u16),
    // Space for future supported networks(e.g. ethereum, bitcoin)
    _Missing,
}

impl Default for Network {
    fn default() -> Self {
        #[cfg(feature = "substrate")]
        let net = Network::Substrate(42);
        #[cfg(not(feature = "substrate"))]
        let net = Network::_Missing;
        net
    }
}

impl core::fmt::Display for Network {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            #[cfg(feature = "substrate")]
            Self::Substrate(_) => write!(f, "substrate"),
            _ => write!(f, ""),
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
            #[cfg(feature = "mnemonic")]
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

mod util {
    #[cfg(feature = "rand")]
    pub fn random_bytes<R, const S: usize>(rng: &mut R) -> [u8; S]
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let mut bytes = [0u8; S];
        rng.fill_bytes(&mut bytes);
        bytes
    }
}
