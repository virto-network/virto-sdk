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
use core::fmt;

pub use key_pair::{any, Derive, Pair, Public, Signature, Signer, SigningError};
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
    pub async fn unlock(
        &mut self,
        account: V::Id,
        cred: impl Into<V::Credentials>,
    ) -> Result<(), Error<V::Error>> {
        if self.is_locked() {
            let vault = &mut self.vault;
            let signer = vault.unlock(account, cred).await.map_err(Error::Vault)?;

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
    pub async fn sign(&self, message: &[u8]) -> Result<impl Signature, SigningError> {
        if self.is_locked() {
            return Err(SigningError::Locked);
        }

        let signer = self.default_account().ok_or(SigningError::NoAccount)?;

        signer.sign_msg(message).await
    }

    /// Save data to be signed some time later.
    /// Returns an error if the message exceeds the maximum size.
    pub fn sign_later<T>(&mut self, message: T) -> Result<(), Error<V::Error>>
    where
        T: AsRef<[u8]>,
    {
        let msg = message.as_ref();
        let msg = msg.try_into().map_err(|_| Error::MessageTooLong)?;
        self.pending_sign.push((msg, None));
        Ok(())
    }

    /// Try to sign all messages in the queue returning the list of signatures.
    pub async fn sign_pending(&mut self) -> Result<ArrayVec<impl AsRef<[u8]>, M>, SigningError> {
        let mut signatures = ArrayVec::new();
        for (msg, a) in self.pending_sign.take() {
            let signer = match a {
                Some(idx) => self.account(idx),
                None => self.default_account().ok_or(SigningError::NoAccount)?,
            };

            let message = signer.sign_msg(&msg).await?;
            signatures.push(message);
        }
        Ok(signatures)
    }

    /// Iterate over the messages pending for signature.
    pub fn pending(&self) -> impl Iterator<Item = &[u8]> {
        self.pending_sign.iter().map(|(msg, _)| msg.as_ref())
    }

    fn account(&self, idx: u8) -> &V::Account {
        &self.accounts[idx as usize]
    }
}

/// Represents the blockchain network in use by an account
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Network {
    /// Substrate-based blockchains, distinguished by SS58 address prefix.
    /// 42 is the generic prefix.
    Substrate(u16),
}

impl Default for Network {
    fn default() -> Self {
        Network::Substrate(42)
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Substrate(_) => write!(f, "substrate"),
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum Error<V> {
    Vault(V),
    Locked,
    DeriveError,
    MessageTooLong,
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
            Error::MessageTooLong => write!(f, "Message exceeds max size of {} bytes", MSG_MAX_SIZE),
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
