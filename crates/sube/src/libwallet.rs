//! Adapter for using a `libwallet` signer with ordinary V4 transactions.
//!
//! This optional module intentionally contains no pallet-pass knowledge.

use crate::{Error, Result, Signer};
use libwallet::Signer as WalletSigner;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureScheme {
    Sr25519,
    Ed25519,
    Ecdsa,
}

impl SignatureScheme {
    fn metadata_name(self) -> &'static str {
        match self {
            Self::Sr25519 => "Sr25519",
            Self::Ed25519 => "Ed25519",
            Self::Ecdsa => "Ecdsa",
        }
    }
}

/// Associates a libwallet signer with its on-chain AccountId32.
///
/// Keeping the account explicit also supports hardware/remote signers whose
/// generic signing trait deliberately does not export a private or public key.
pub struct LibwalletSigner<S> {
    signer: S,
    account: [u8; 32],
    scheme: SignatureScheme,
}

impl<S> LibwalletSigner<S> {
    pub fn new(signer: S, account: [u8; 32], scheme: SignatureScheme) -> Self {
        Self {
            signer,
            account,
            scheme,
        }
    }

    pub fn inner(&self) -> &S {
        &self.signer
    }

    pub fn into_inner(self) -> S {
        self.signer
    }
}

impl<S: WalletSigner> Signer for LibwalletSigner<S> {
    type Account = [u8; 32];
    type Signature = S::Signature;

    async fn sign(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature> {
        self.signer
            .sign_msg(data)
            .await
            .map_err(|error| Error::Signing(error.to_string()))
    }

    fn account(&self) -> Self::Account {
        self.account
    }

    fn signature_variant(&self) -> Option<&str> {
        Some(self.scheme.metadata_name())
    }
}
