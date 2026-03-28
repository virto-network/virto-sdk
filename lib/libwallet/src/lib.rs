#![cfg_attr(not(any(test, feature = "std")), no_std)]
//! `libwallet` is a high-level wallet abstraction that manages accounts
//! backed by any signer implementation — from vault-derived keypairs to
//! hardware wallets and remote signers.
#[cfg(not(any(feature = "sr25519")))]
compile_error!("Enable at least one type of signature algorithm");

mod account;
mod key_pair;
pub mod util;

#[cfg(feature = "substrate")]
mod substrate_ext;
#[cfg(feature = "substrate")]
pub use substrate_ext::substrate_seed;

pub use account::Account;
use arrayvec::ArrayVec;
use core::fmt;

pub use key_pair::{any, Derive, Pair, Public, Signature, Signer, SigningError};
#[cfg(feature = "mnemonic")]
pub use mnemonic::{Language, Mnemonic};
pub mod vault;

const MSG_MAX_SIZE: usize = u8::MAX as usize;
type Message = ArrayVec<u8, { MSG_MAX_SIZE }>;

/// Wallet manages a collection of named accounts, each wrapping any signer.
///
/// Accounts can come from vaults, hardware wallets, browser extensions, or
/// any other source that implements `Signer`.
pub struct Wallet<S: Signer, const A: usize = 5, const M: usize = A> {
    default_account: Option<u8>,
    accounts: ArrayVec<Account<S>, A>,
    pending_sign: ArrayVec<(Message, Option<u8>), M>,
}

impl<S: Signer + fmt::Debug> fmt::Debug for Wallet<S, 5, 5> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wallet")
            .field("accounts", &self.accounts.len())
            .field("default", &self.default_account)
            .field("pending", &self.pending_sign.len())
            .finish()
    }
}

impl<S, const A: usize, const M: usize> Wallet<S, A, M>
where
    S: Signer,
{
    /// Create a new empty wallet.
    pub fn new() -> Self {
        Wallet {
            default_account: None,
            accounts: ArrayVec::new_const(),
            pending_sign: ArrayVec::new(),
        }
    }

    /// Add an account to the wallet. The first account added becomes the default.
    pub fn add(&mut self, name: &str, signer: S) -> usize {
        let idx = self.accounts.len();
        self.accounts.push(Account::new(name, signer));
        if self.default_account.is_none() {
            self.default_account = Some(idx as u8);
        }
        idx
    }

    /// Set the default account by index.
    pub fn set_default(&mut self, idx: usize) {
        if idx < self.accounts.len() {
            self.default_account = Some(idx as u8);
        }
    }

    /// Get the account currently set as default.
    pub fn default_account(&self) -> Option<&Account<S>> {
        self.default_account
            .and_then(|x| self.accounts.get(x as usize))
    }

    /// Get an account by index.
    pub fn account(&self, idx: usize) -> Option<&Account<S>> {
        self.accounts.get(idx)
    }

    /// Number of accounts in the wallet.
    pub fn accounts(&self) -> usize {
        self.accounts.len()
    }

    /// Clear all accounts, triggering zeroization via Drop.
    pub fn lock(&mut self) {
        self.accounts.clear();
        self.default_account = None;
    }

    /// Sign a message with the default account.
    pub async fn sign(&self, message: &[u8]) -> Result<impl Signature, SigningError> {
        let signer = self.default_account().ok_or(SigningError::NoAccount)?;
        signer.sign_msg(message).await
    }

    /// Save data to be signed some time later.
    /// Returns an error if the message exceeds the maximum size.
    pub fn sign_later<T>(&mut self, message: T) -> Result<(), Error>
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
                Some(idx) => self.accounts.get(idx as usize).ok_or(SigningError::NoAccount)?,
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
pub enum Error {
    MessageTooLong,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::MessageTooLong => write!(f, "Message exceeds max size of {} bytes", MSG_MAX_SIZE),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}
