#![cfg_attr(not(any(test, feature = "std")), no_std)]
//! `libwallet` is the one-stop tool to build easy, slightly opinionated crypto wallets
//! that run in all kinds of environments and plattforms including embedded hardware,
//! mobile apps or the Web.
//! It's easy to extend implementing different vault backends and it's designed to
//! be compatible with all kinds of key formats found in many different blockchains.
#[cfg(not(any(feature = "sr25519")))]
compile_error!("Enable at least one type of signature algorithm");

mod account;
mod key_pair;
pub mod util;

#[cfg(feature = "substrate")]
mod substrate_ext;

pub use account::Account;
use arrayvec::ArrayVec;
use core::{cell::RefCell, convert::TryInto, fmt};
use key_pair::any::AnySignature;

#[cfg(feature = "mnemonic")]
use mnemonic;

pub use key_pair::*;
#[cfg(feature = "mnemonic")]
pub use mnemonic::{Language, Mnemonic};
pub use vault::Vault;
pub mod vault;


const MSG_MAX_SIZE: usize = u8::MAX as usize;
type Message = ArrayVec<u8, { MSG_MAX_SIZE }>;

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
pub struct Wallet<V: Vault, const A: usize = 5, const M: usize = A> {
    vault: V,
    is_locked: bool,
    default_account: Option<u8>,
    accounts: ArrayVec<V::Account, A>,
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
            default_account: None,
            accounts: ArrayVec::new_const(),
            pending_sign: ArrayVec::new(),
            is_locked: true,
        }
    }

    /// Get the account currently set as default
    pub fn default_account(&self) -> Option<&V::Account> {
        self.default_account.map(|x| &self.accounts[x as usize])
    }

    /// Use credentials to unlock the vault.
    ///
    /// ```
    /// # use libwallet::{Wallet, Error, vault, Vault};
    /// # use std::convert::TryInto;
    /// # type SimpleVault = vault::Simple<String>;
    /// # type Result = std::result::Result<(), Error<<SimpleVault as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = SimpleVault::generate(&mut rand_core::OsRng);
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// if wallet.is_locked() {
    ///     wallet.unlock(None, None).await?;
    /// }
    ///
    /// assert!(!wallet.is_locked());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock(
        &mut self,
        account: V::Id,
        cred: impl Into<V::Credentials>,
    ) -> Result<(), Error<V::Error>> {
        if self.is_locked() {
            let vault = &mut self.vault;

            let signer = vault
                .unlock(account, cred)
                .await
                .map_err(|e| Error::Vault(e))?;

            if self.default_account.is_none() {
                self.default_account = Some(0);
            }

            self.accounts.push(signer);

            self.is_locked = false;
        }
        Ok(())
    }

    /// Check if the vault has been unlocked.
    pub fn is_locked(&self) -> bool {
        self.is_locked
    }

    /// Sign a message with the default account and return the signature.
    /// The wallet needs to be unlocked.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error, Signer, Vault};
    /// # type SimpleVault = vault::Simple<String>;
    /// # type Result = std::result::Result<(), Error<<SimpleVault as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = SimpleVault::generate(&mut rand_core::OsRng);
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.unlock(None, None).await?;
    ///
    /// let msg = &[0x12, 0x34, 0x56];
    /// let signature = wallet.sign(msg).await.expect("it must sign");
    ///
    /// assert!(wallet.default_account().expect("it must have a default signer").verify(msg, signature.as_ref()).await);
    /// # Ok(()) }
    /// ```
    pub async fn sign(&self, message: &[u8]) -> Result<impl Signature, ()> {
        assert!(!self.is_locked());

        let Some(signer) = self.default_account() else {
            return Err(());
        };

        signer.sign_msg(message).await
    }

    /// Save data to be signed some time later.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error, Vault};
    /// # type SimpleVault = vault::Simple<String>;
    /// # type Result = std::result::Result<(), Error<<SimpleVault as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = SimpleVault::generate(&mut rand_core::OsRng);
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
    /// # use libwallet::{Wallet, vault, Error, Vault};
    /// # type SimpleVault = vault::Simple<String>;
    /// # type Result = std::result::Result<(), Error<<SimpleVault as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = SimpleVault::generate(&mut rand_core::OsRng);
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.unlock(None, None).await?;
    ///
    /// wallet.sign_later(&[0x01, 0x02, 0x03]);
    /// wallet.sign_later(&[0x04, 0x05, 0x06]);
    /// let signatures = wallet.sign_pending().await.expect("it must sign");
    ///
    /// assert_eq!(signatures.len(), 2);
    /// assert_eq!(wallet.pending().count(), 0);
    /// # Ok(()) }
    /// ```
    pub async fn sign_pending(&mut self) -> Result<ArrayVec<impl AsRef<[u8]>, M>, ()> {
        let mut signatures = ArrayVec::new();
        for (msg, a) in self.pending_sign.take() {
            let signer = a
                .map(|idx| self.account(idx))
                .unwrap_or_else(|| self.default_account().expect("Signer not set"));

            let message = signer.sign_msg(&msg).await?;
            signatures.push(message);
        }
        Ok(signatures)
    }

    /// Iteratate over the messages pending for signature for all the accounts.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error, Vault};
    /// # type SimpleVault = vault::Simple<String>;
    /// # type Result = std::result::Result<(), Error<<SimpleVault as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = SimpleVault::generate(&mut rand_core::OsRng);
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

    fn account(&self, idx: u8) -> &V::Account {
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

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            #[cfg(feature = "substrate")]
            Self::Substrate(_) => write!(f, "substrate"),
            _ => write!(f, ""),
        }
    }
}

#[derive(Debug)]
pub enum Error<V> {
    Vault(V),
    Locked,
    DeriveError,
    #[cfg(feature = "mnemonic")]
    InvalidPhrase,
}

impl<V> fmt::Display for Error<V>
where
    V: fmt::Debug + fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Vault(e) => write!(f, "Vault error: {}", e),
            Error::Locked => write!(f, "Locked"),
            Error::DeriveError => write!(f, "Cannot derive"),
            #[cfg(feature = "mnemonic")]
            Error::InvalidPhrase => write!(f, "Invalid phrase"),
        }
    }
}

#[cfg(feature = "std")]
impl<V> std::error::Error for Error<V> where V: fmt::Debug + fmt::Display {}

#[cfg(feature = "mnemonic")]
impl<V> From<mnemonic::Error> for Error<V> {
    fn from(_: mnemonic::Error) -> Self {
        Error::InvalidPhrase
    }
}

