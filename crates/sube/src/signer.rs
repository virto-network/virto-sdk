use crate::Result;
use crate::extrinsic::{self, ChainContext};
use crate::metadata::ExtrinsicMeta;
use crate::prelude::*;
use crate::value::DynValue;
use core::{future::Future, marker::PhantomData};

pub type Bytes<const N: usize> = [u8; N];

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
}

/// Strategy for assembling a complete extrinsic from an encoded call.
///
/// Two kinds of extrinsics are supported:
/// - **V4 Signed**: traditional account + cryptographic signature (via [`Signer`] blanket impl)
/// - **V5 General**: extension-based authentication (e.g. pallet-pass `PassAuthenticate`)
///
/// The builder accepts any `ExtrinsicAssembler` where `.signer(s)` is called.
#[allow(async_fn_in_trait)]
pub trait ExtrinsicAssembler {
    type Account: AsRef<[u8]>;

    /// Account identifier used for nonce lookup.
    fn account(&self) -> Self::Account;

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
    ) -> Result<Vec<u8>>;
}

// Every `Signer` is an `ExtrinsicAssembler` that produces V4 signed extrinsics.
impl<T: Signer> ExtrinsicAssembler for T {
    type Account = T::Account;

    fn account(&self) -> Self::Account {
        Signer::account(self)
    }

    async fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &scales::Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> Result<Vec<u8>> {
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
