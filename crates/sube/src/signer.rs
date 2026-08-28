use crate::Result;
use crate::extrinsic::{self, AssembledExtrinsic, AuthorizationSummary, ChainContext};
use crate::metadata::ExtrinsicMeta;
use crate::prelude::*;
use crate::value::DynValue;
use core::{future::Future, marker::PhantomData};

pub type Bytes<const N: usize> = [u8; N];

/// Cryptographic signature variant used by a V4 Substrate extrinsic.
///
/// The variant name is resolved through runtime metadata; no enum
/// discriminant is hard-coded by Sube.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureScheme {
    Sr25519,
    Ed25519,
    Ecdsa,
}

impl SignatureScheme {
    /// Variant name conventionally exposed by Substrate's `MultiSignature`.
    pub const fn metadata_name(self) -> &'static str {
        match self {
            Self::Sr25519 => "Sr25519",
            Self::Ed25519 => "Ed25519",
            Self::Ecdsa => "Ecdsa",
        }
    }

    /// Exact encoded signature length expected for this scheme.
    pub const fn signature_len(self) -> usize {
        match self {
            Self::Sr25519 | Self::Ed25519 => 64,
            Self::Ecdsa => 65,
        }
    }
}

/// Signed extrinsics need to be signed by a `Signer` before submission.
///
/// Implementors provide a cryptographic signature over raw bytes (V4 signed extrinsics).
/// Every `Signer` automatically implements [`ExtrinsicAssembler`] via a blanket impl
/// that produces V4 signed extrinsics.
#[allow(async_fn_in_trait)]
pub trait Signer {
    type Account: AsRef<[u8]>;
    type Signature: AsRef<[u8]>;

    async fn sign(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature>;

    fn account(&self) -> Self::Account;

    /// Name of the signature variant expected by the runtime's metadata.
    ///
    /// Ordinary Substrate signers default to `Sr25519`. Ed25519 and ECDSA
    /// adapters should override this instead of relying on a hard-coded
    /// discriminant in the extrinsic encoder.
    fn signature_variant(&self) -> Option<&str> {
        Some("Sr25519")
    }
}

/// Strategy for assembling a complete extrinsic from an encoded call.
///
/// Two kinds of extrinsics are supported:
/// - **V4 Signed**: traditional account + cryptographic signature (via [`Signer`] blanket impl)
/// - **V5 General**: extension-based authentication (e.g. pallet-pass `PassAuthenticate`)
///
/// [`Sube::build_transaction`](crate::Sube::build_transaction) accepts any
/// `ExtrinsicAssembler`.
#[allow(async_fn_in_trait)]
pub trait ExtrinsicAssembler {
    type Account: AsRef<[u8]>;

    /// Account identifier used for nonce lookup.
    fn nonce_account(&self) -> Self::Account {
        #[allow(deprecated)]
        self.account()
    }

    /// Compatibility alias for assemblers written before the nonce/signing
    /// identities were separated. New implementations should override
    /// [`nonce_account`](Self::nonce_account).
    #[deprecated(note = "use nonce_account; signing identity may be different")]
    fn account(&self) -> Self::Account {
        self.nonce_account()
    }

    /// Human-readable authorization identities used by transaction review.
    ///
    /// A session assembler can override this so `signing_account` is the
    /// session key while `nonce_account` remains the pass account.
    fn authorization(&self) -> AuthorizationSummary {
        let nonce_account = self.nonce_account().as_ref().to_vec();
        AuthorizationSummary {
            signing_account: nonce_account.clone(),
            nonce_account,
            scheme: None,
        }
    }

    /// Assemble the extrinsic inner bytes (before the SCALE compact length prefix).
    ///
    /// Receives the SCALE-encoded call, extrinsic metadata, type registry,
    /// chain context (spec version, genesis hash, nonce, etc.), and any
    /// user-provided extension overrides.
    async fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &scales::Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> Result<AssembledExtrinsic>;
}

// Every `Signer` is an `ExtrinsicAssembler` that produces V4 signed extrinsics.
impl<T: Signer> ExtrinsicAssembler for T {
    type Account = T::Account;

    fn nonce_account(&self) -> Self::Account {
        Signer::account(self)
    }

    #[allow(deprecated)]
    fn account(&self) -> Self::Account {
        Signer::account(self)
    }

    fn authorization(&self) -> AuthorizationSummary {
        let account = Signer::account(self).as_ref().to_vec();
        AuthorizationSummary {
            signing_account: account.clone(),
            nonce_account: account,
            scheme: self.signature_variant().map(Into::into),
        }
    }

    async fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &scales::Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> Result<AssembledExtrinsic> {
        extrinsic::assemble_signed_v4(self, encoded_call, meta, registry, ctx, overrides).await
    }
}

/// Adapter that implements [`Signer`] from a 32-byte account and a signing closure.
/// Construct via `SignerFn::new(account, |data| async { ... })`.
pub struct SignerFn<S, SF> {
    account: Bytes<32>,
    signer: S,
    _fut: PhantomData<SF>,
}

impl<S, SF> Signer for SignerFn<S, SF>
where
    S: Fn(&[u8]) -> SF,
    SF: Future<Output = Result<Bytes<64>>>,
{
    type Account = Bytes<32>;
    type Signature = Bytes<64>;

    async fn sign(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature> {
        (self.signer)(data.as_ref()).await
    }

    fn account(&self) -> Self::Account {
        self.account
    }
}

impl<T: Signer> Signer for &T {
    type Account = T::Account;
    type Signature = T::Signature;

    async fn sign(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature> {
        (*self).sign(data).await
    }

    fn account(&self) -> Self::Account {
        (*self).account()
    }

    fn signature_variant(&self) -> Option<&str> {
        (*self).signature_variant()
    }
}

impl<S, SF> SignerFn<S, SF> {
    pub fn new(account: Bytes<32>, signer: S) -> Self {
        Self {
            account,
            signer,
            _fut: PhantomData,
        }
    }
}

impl<S, SF> From<(Bytes<32>, S)> for SignerFn<S, SF> {
    fn from((account, signer): (Bytes<32>, S)) -> Self {
        Self::new(account, signer)
    }
}
