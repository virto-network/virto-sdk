#![cfg_attr(not(any(test, feature = "std")), no_std)]
//! `libwallet` is a high-level wallet abstraction that manages accounts
//! backed by any signer implementation — from vault-derived keypairs to
//! hardware wallets and remote signers.
//!
//! For cross-chain flows, use [`Wallet::sign_with`] on multiple typed
//! wallets to batch-sign messages across different chains.
#[cfg(not(any(feature = "sr25519")))]
compile_error!("Enable at least one type of signature algorithm");

mod account;
mod key_pair;
pub mod util;

#[cfg(feature = "substrate")]
pub mod substrate_ext;
#[cfg(feature = "substrate")]
pub use substrate_ext::{substrate_seed, KeyStore, Substrate};

pub use account::Account;
use arrayvec::ArrayVec;
use core::fmt;

pub use key_pair::{any, Derive, Pair, Public, Signature, Signer, SigningError};
#[cfg(feature = "mnemonic")]
pub use mnemonic::{Language, Mnemonic};
pub mod vault;

/// Wallet manages a collection of named accounts backed by a single signer type.
///
/// For multi-chain workflows involving different signer types, create one wallet
/// per chain and sign across them:
///
/// ```ignore
/// let sub_wallet: Wallet<DerivedSigner> = ...;
/// let eth_wallet: Wallet<LedgerSigner> = ...;
///
/// // sign across chains
/// let sub_sig = sub_wallet.sign(substrate_tx).await?;
/// let eth_sig = eth_wallet.sign_with(0, eth_approval).await?;
/// ```
pub struct Wallet<S: Signer, const A: usize = 5> {
    default_account: Option<u8>,
    accounts: ArrayVec<Account<S>, A>,
}

impl<S: Signer + fmt::Debug, const A: usize> fmt::Debug for Wallet<S, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wallet")
            .field("accounts", &self.accounts.len())
            .field("default", &self.default_account)
            .finish()
    }
}

impl<S, const A: usize> Wallet<S, A>
where
    S: Signer,
{
    /// Create a new empty wallet.
    pub fn new() -> Self {
        Wallet {
            default_account: None,
            accounts: ArrayVec::new_const(),
        }
    }

    /// Add a signer to the wallet. The account name comes from `signer.account_id()`.
    /// The first account added becomes the default.
    pub fn add(&mut self, signer: S) -> &mut Self {
        let idx = self.accounts.len() as u8;
        self.accounts.push(Account::new(signer));
        if self.default_account.is_none() {
            self.default_account = Some(idx);
        }
        self
    }

    /// Set the default account by index.
    pub fn set_default(&mut self, idx: usize) -> &mut Self {
        if idx < self.accounts.len() {
            self.default_account = Some(idx as u8);
        }
        self
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

    /// Find an account by name, returns its index and reference.
    pub fn find(&self, name: &str) -> Option<(usize, &Account<S>)> {
        self.accounts.iter().enumerate()
            .find(|(_, a)| a.name() == name)
    }

    /// Number of accounts in the wallet.
    pub fn accounts_len(&self) -> usize {
        self.accounts.len()
    }

    /// Clear all accounts, triggering zeroization via Drop.
    pub fn lock(&mut self) {
        self.accounts.clear();
        self.default_account = None;
    }

    /// Sign a message with the default account.
    pub async fn sign(&self, message: &[u8]) -> Result<S::Signature, SigningError> {
        self.default_account()
            .ok_or(SigningError::NoAccount)?
            .sign_msg(message)
            .await
    }

    /// Sign a message with a specific account by index.
    pub async fn sign_with(&self, account: usize, message: &[u8]) -> Result<S::Signature, SigningError> {
        self.accounts
            .get(account)
            .ok_or(SigningError::NoAccount)?
            .sign_msg(message)
            .await
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
