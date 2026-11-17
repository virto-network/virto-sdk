#![no_std]
#![allow(async_fn_in_trait)]
//! pallet-pass authenticator for sube.
//!
//! Produces V5 "General" extrinsics authenticated via the `PassAuthenticate`
//! transaction extension, as used by the Kreivo blockchain.
//!
//! # Usage
//!
//! Any [`DeviceAuthenticator`] (a substrate key, a WebAuthn authenticator,
//! ...) authenticates calls for the pass account it was registered to:
//!
//! ```rust,ignore
//! use pass::{Account, DeviceId, HashedUserId, PassAuthenticator, PassRuntimeConfig};
//! use pass::wallet::{SignatureType, WalletDevice};
//!
//! let config = PassRuntimeConfig::discover(chain.metadata())?;
//! let device = WalletDevice::new(&signer, signer.public(), SignatureType::Sr25519);
//! let checkpoint = chain.backend().block_info(None).await?;
//! let auth = PassAuthenticator::new(
//!     &device,
//!     config.derive_account(user_id),
//!     device.device_id(),
//!     user_id,
//!     config.authority_id,
//!     checkpoint.number as u32,
//!     checkpoint.hash,
//! );
//!
//! let call = chain.prepare_call("balances/transfer", &sube::Text("(...)"))?;
//! let extrinsic = chain
//!     .build_transaction(&call, &auth, sube::TransactionOptions::default())
//!     .await?;
//! ```

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod config;
mod filter;
pub mod session;
#[cfg(feature = "wallet")]
pub mod wallet;
pub mod webauthn;
pub mod workflow;

mod newtypes;
pub use config::PassRuntimeConfig;
pub use filter::{CallIndex, DeviceFilter};
pub use newtypes::{Account, AuthorityId, DeviceId, HashedUserId, InvalidHashedUserId};
pub use session::SessionAuthorizer;
pub use workflow::{DeviceAuthenticator, EnrollmentDraft};

use alloc::string::String;
use alloc::vec::Vec;

use blake2::{Blake2b256, Digest};
use sube::extrinsic::{AssembledExtrinsic, ChainContext, encode_extensions};
use sube::metadata::{ExtrinsicMeta, SignedExtensionMeta};
use sube::{DynValue, Error, ExtrinsicAssembler, Registry, Result};
use workflow::ChallengeRequest;

/// Fixed-size challenge produced by pallet-pass challengers.
pub type Challenge = [u8; 32];

/// Compute Kreivo's block challenger:
/// `blake2_256(block_hash(context) || binding)`.
///
/// The caller must resolve the context block to its hash. Hashing the encoded
/// block number here would be incorrect and would never match the runtime.
pub fn block_challenge(block_hash: &[u8; 32], binding: &[u8]) -> Challenge {
    let mut input = Vec::with_capacity(64);
    input.extend_from_slice(block_hash);
    input.extend_from_slice(binding);
    Blake2b256::digest(&input).into()
}

/// Enrollment binds the challenge to the derived AccountId32 (whose SCALE
/// encoding is its 32 bytes).
pub fn enrollment_challenge(block_hash: &[u8; 32], account: Account) -> Challenge {
    block_challenge(block_hash, &account.0)
}

/// The first byte of a V5 General extrinsic: the `0b01` General preamble
/// and format version 5. It is fixed, since V14/V15 metadata only
/// advertises version 4.
const GENERAL_V5: u8 = 0b0100_0101;
/// The transaction-extension version. Runtimes that don't version their
/// extensions use 0.
const EXTENSION_VERSION: u8 = 0;
/// Extension identifier used by pallet-pass.
const PASS_AUTHENTICATE: &str = "PassAuthenticate";

/// pallet-pass authenticator that produces V5 "General" extrinsics.
///
/// Implements [`ExtrinsicAssembler`] by encoding the `PassAuthenticate`
/// transaction extension with a credential from a [`DeviceAuthenticator`],
/// challenged with [`block_challenge`] over the extrinsic's inherited
/// implication. All SCALE encoding is delegated to `scales` via the type
/// registry.
pub struct PassAuthenticator<'a, A> {
    device: &'a A,
    account: Account,
    device_id: DeviceId,
    user_id: HashedUserId,
    authority_id: AuthorityId,
    context: u32,
    block_hash: [u8; 32],
}

impl<'a, A> PassAuthenticator<'a, A> {
    /// Authenticate as `device_id` of `account`, against the challenge
    /// context `context` whose block hash is `block_hash`.
    ///
    /// The newtype wrappers prevent argument-swap bugs.
    pub fn new(
        device: &'a A,
        account: Account,
        device_id: DeviceId,
        user_id: HashedUserId,
        authority_id: AuthorityId,
        context: u32,
        block_hash: [u8; 32],
    ) -> Self {
        Self {
            device,
            account,
            device_id,
            user_id,
            authority_id,
            context,
            block_hash,
        }
    }
}

impl<A: DeviceAuthenticator> ExtrinsicAssembler for PassAuthenticator<'_, A> {
    type Account = [u8; 32];

    fn nonce_account(&self) -> Self::Account {
        self.account.0
    }

    async fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> Result<AssembledExtrinsic> {
        let after = extensions_after_pass_authenticate(&meta.extensions)?;

        // Encode extensions AFTER PassAuthenticate to compute the inherited implication.
        let (after_extra, after_additional, _) =
            encode_extensions(after, registry, ctx, overrides)?;

        let implication = inherited_implication(
            EXTENSION_VERSION,
            encoded_call,
            &after_extra,
            &after_additional,
        );

        // Ask the device for the credential bound to this implication.
        let credential_value = self
            .device
            .assert(&ChallengeRequest {
                user_id: self.user_id,
                authority_id: self.authority_id,
                context: self.context,
                challenge: block_challenge(&self.block_hash, &implication),
            })
            .await?;

        // `PassAuthenticate(Option<AuthenticateParams>)`: a bare value for an
        // `Option` is encoded as `Some(value)`.
        let pass_value = DynValue::obj(&[
            ("device_id", DynValue::from(self.device_id.0)),
            ("credential", credential_value),
        ]);

        // Re-encode ALL extensions with the PassAuthenticate override in place.
        let mut all_overrides = Vec::from(overrides);
        all_overrides.push((PASS_AUTHENTICATE.into(), pass_value));
        let (all_extra, _, extensions) =
            encode_extensions(&meta.extensions, registry, ctx, &all_overrides)?;

        // Assemble the V5 General extrinsic: [ver, ext_ver, extras, call]
        let mut inner = Vec::with_capacity(2 + all_extra.len() + encoded_call.len());
        inner.push(GENERAL_V5);
        inner.push(EXTENSION_VERSION);
        inner.extend_from_slice(&all_extra);
        inner.extend_from_slice(encoded_call);

        Ok(AssembledExtrinsic {
            bytes: inner,
            extensions,
            signing_account: self.device_id.0.to_vec(),
            scheme: PASS_AUTHENTICATE.into(),
        })
    }
}

/// The extensions after `PassAuthenticate`, which its credential binds to.
fn extensions_after_pass_authenticate(
    exts: &[SignedExtensionMeta],
) -> Result<&[SignedExtensionMeta]> {
    let idx = exts
        .iter()
        .position(|e| e.identifier == PASS_AUTHENTICATE)
        .ok_or_else(|| {
            Error::MissingExtensionValue("PassAuthenticate not found in metadata".into())
        })?;
    Ok(&exts[idx + 1..])
}

/// Compute the inherited implication that the signer binds to:
/// `blake2_256(extension_version || encoded_call || after_extras || after_additionals)`.
///
/// This is Substrate's `TxBaseImplication((extension_version, call))`
/// followed by the extensions after `PassAuthenticate`, which pallet-pass
/// hashes with `using_encoded(blake2_256)`.
pub fn inherited_implication(
    extension_version: u8,
    encoded_call: &[u8],
    after_extra: &[u8],
    after_additional: &[u8],
) -> [u8; 32] {
    let mut raw =
        Vec::with_capacity(1 + encoded_call.len() + after_extra.len() + after_additional.len());
    raw.push(extension_version);
    raw.extend_from_slice(encoded_call);
    raw.extend_from_slice(after_extra);
    raw.extend_from_slice(after_additional);
    Blake2b256::digest(&raw).into()
}

#[cfg(test)]
mod challenge_tests {
    use super::*;

    #[test]
    fn block_challenge_deterministic() {
        let binding = [0xab; 32];
        assert_eq!(
            block_challenge(&[42; 32], &binding),
            block_challenge(&[42; 32], &binding)
        );
    }

    #[test]
    fn block_challenge_varies_with_block_hash() {
        let binding = [0xab; 32];
        assert_ne!(
            block_challenge(&[1; 32], &binding),
            block_challenge(&[2; 32], &binding)
        );
    }

    #[test]
    fn block_challenge_varies_with_binding() {
        assert_ne!(
            block_challenge(&[1; 32], &[0x01; 32]),
            block_challenge(&[1; 32], &[0x02; 32])
        );
    }

    #[test]
    fn kreivo_challenge_golden_vector() {
        assert_eq!(
            hex::encode(block_challenge(&[0x11; 32], &[0x22; 32])),
            "428d37d6b34f605a8ff32b6a04c95d9c7d2aead9ecde193b2f5019b7f13ced23"
        );
    }

    #[test]
    fn enrollment_challenge_binds_the_account_bytes() {
        assert_eq!(
            hex::encode(enrollment_challenge(&[0x11; 32], Account([0x22; 32]))),
            "428d37d6b34f605a8ff32b6a04c95d9c7d2aead9ecde193b2f5019b7f13ced23"
        );
    }
}
