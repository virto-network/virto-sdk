use crate::extrinsic::{self, ChainContext};
use crate::metadata::ExtrinsicMeta;
use crate::prelude::*;
use crate::value::DynValue;
use crate::Result;
use core::{future::Future, marker::PhantomData};

pub type Bytes<const N: usize> = [u8; N];

/// Signed extrinsics need to be signed by a `Signer` before submission.
///
/// Implementors provide a cryptographic signature over raw bytes (V4 signed extrinsics).
/// Every `Signer` automatically implements [`ExtrinsicAssembler`] via a blanket impl
/// that produces V4 signed extrinsics.
pub trait Signer {
    type Account: AsRef<[u8]>;
    type Signature: AsRef<[u8]>;

    fn sign(&self, data: impl AsRef<[u8]>) -> impl Future<Output = Result<Self::Signature>>;

    fn account(&self) -> Self::Account;
}

/// Strategy for assembling a complete extrinsic from an encoded call.
///
/// Two kinds of extrinsics are supported:
/// - **V4 Signed**: traditional account + cryptographic signature (via [`Signer`] blanket impl)
/// - **V5 General**: extension-based authentication (e.g. pallet-pass `PassAuthenticate`)
///
/// The builder accepts any `ExtrinsicAssembler` where `.signer(s)` is called.
pub trait ExtrinsicAssembler {
    type Account: AsRef<[u8]>;

    /// Account identifier used for nonce lookup.
    fn account(&self) -> Self::Account;

    /// Assemble the extrinsic inner bytes (before the SCALE compact length prefix).
    ///
    /// Receives the SCALE-encoded call, extrinsic metadata, type registry,
    /// chain context (spec version, genesis hash, nonce, etc.), and any
    /// user-provided extension overrides.
    fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &scales::Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> impl Future<Output = Result<Vec<u8>>>;
}

// Every `Signer` is an `ExtrinsicAssembler` that produces V4 signed extrinsics.
impl<T: Signer> ExtrinsicAssembler for T {
    type Account = T::Account;

    fn account(&self) -> Self::Account {
        Signer::account(self)
    }

    fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &scales::Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> impl Future<Output = Result<Vec<u8>>> {
        async move {
            extrinsic::assemble_signed_v4(self, encoded_call, meta, registry, ctx, overrides).await
        }
    }
}

/// Adapter that implements [`Signer`] from a 32-byte account and a signing closure.
/// Construct via `SignerFn::from((account, |data| async { ... }))`.
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

    fn sign(&self, data: impl AsRef<[u8]>) -> impl Future<Output = Result<Self::Signature>> {
        (self.signer)(data.as_ref())
    }

    fn account(&self) -> Self::Account {
        self.account
    }
}

impl<T: Signer> Signer for &T {
    type Account = T::Account;
    type Signature = T::Signature;

    fn sign(&self, data: impl AsRef<[u8]>) -> impl Future<Output = Result<Self::Signature>> {
        (*self).sign(data)
    }

    fn account(&self) -> Self::Account {
        (*self).account()
    }
}

impl<A: AsRef<[u8]>, S, SF> From<(A, S)> for SignerFn<S, SF>
where
    A: AsRef<[u8]>,
    S: Fn(&[u8]) -> SF,
{
    fn from((account, signer): (A, S)) -> Self {
        SignerFn {
            account: account.as_ref().try_into().expect("32bit account"),
            signer,
            _fut: PhantomData,
        }
    }
}
