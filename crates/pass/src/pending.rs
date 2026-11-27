//! A pass transaction whose device answers later.

use alloc::string::String;
use alloc::vec::Vec;

use sube::extrinsic::{ChainContext, EncodedExtrinsic, PreparedCall};
use sube::{DynValue, Error, Metadata, Result};

use crate::PassIdentity;
use crate::workflow::ChallengeRequest;

/// A pass transaction waiting for its device's answer.
///
/// [`PassAuthenticator`](crate::PassAuthenticator) asks a device and builds
/// the extrinsic in one call. A server, or a page whose passkey answers in a
/// later request, cannot hold that call open: it makes a `Pending`, hands
/// [`request`](Self::request) to the device, keeps the `Pending`, and calls
/// [`finish`](Self::finish) when the answer arrives.
///
/// The challenge commits to the chain context the `Pending` was made in:
/// its era and the account's nonce. So the answer is good until
/// [`expires_at`](Self::expires_at), and only while the account's nonce is
/// unchanged: any other transaction of the pass account that lands first,
/// one made with a session key included, makes this one stale, and two
/// `Pending`s made from the same context cannot both land.
#[derive(Clone, Debug)]
pub struct Pending {
    /// Who the transaction is authenticated as.
    pub identity: PassIdentity,
    /// The call it dispatches.
    pub call: PreparedCall,
    /// The chain context it was built in.
    pub context: ChainContext,
    /// Extension values given by the caller.
    pub overrides: Vec<(String, DynValue)>,
    /// What the device must answer.
    pub request: ChallengeRequest,
}

impl Pending {
    /// Prepare `call` as `identity` in `context`, up to the point where the
    /// device is asked.
    ///
    /// `context` comes from [`sube::extrinsic::build_context`] for
    /// `identity.account`.
    pub fn new(
        identity: PassIdentity,
        call: PreparedCall,
        context: ChainContext,
        overrides: Vec<(String, DynValue)>,
        metadata: &Metadata,
    ) -> Result<Self> {
        let request = identity.challenge(
            &call.bytes,
            &metadata.extrinsic,
            &metadata.registry,
            &context,
            &overrides,
        )?;
        Ok(Self {
            identity,
            call,
            context,
            overrides,
            request,
        })
    }

    /// The last block this transaction is valid in, or `None` if it does
    /// not expire.
    pub fn expires_at(&self) -> Option<u64> {
        self.context.expires_at()
    }

    /// The extrinsic authenticated by `credential`, the device's answer to
    /// [`request`](Self::request), ready for
    /// [`Sube::submit_transaction`](sube::Sube::submit_transaction).
    ///
    /// Fails if `metadata` no longer gives the challenge the device
    /// answered, as after a runtime upgrade.
    pub fn finish(&self, credential: DynValue, metadata: &Metadata) -> Result<EncodedExtrinsic> {
        let asked = self.identity.challenge(
            &self.call.bytes,
            &metadata.extrinsic,
            &metadata.registry,
            &self.context,
            &self.overrides,
        )?;
        if asked != self.request {
            return Err(Error::Signing(
                "the transaction no longer matches what the device was asked".into(),
            ));
        }
        let assembled = self.identity.assemble_with(
            credential,
            &self.call.bytes,
            &metadata.extrinsic,
            &metadata.registry,
            &self.context,
            &self.overrides,
        )?;
        EncodedExtrinsic::from_assembled(
            assembled,
            &self.call,
            &self.context,
            &self.identity.account.0,
        )
    }
}
