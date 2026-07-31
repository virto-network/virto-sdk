#![no_std]
#![allow(async_fn_in_trait)]
//! pallet-pass authenticator for sube.
//!
//! Produces V5 "General" extrinsics authenticated via the `PassAuthenticate`
//! transaction extension, as used by the Kreivo blockchain.
//!
//! # Usage
//!
//! ```rust,ignore
//! use pass::{PassAuthenticator, Account, DeviceId, HashedUserId, AuthorityId,
//!            CredentialMeta, wallet::{WalletCredential, SignatureType}};
//!
//! let cred = WalletCredential::new(
//!     CredentialMeta::new(
//!         HashedUserId(user_id_hash),
//!         AuthorityId(authority),
//!         block_number,
//!         block_hash,
//!         "SubstrateKey",
//!     ),
//!     SignatureType::Sr25519,
//!     &my_signer,
//! );
//! let auth = PassAuthenticator::new(Account(acc), DeviceId(dev), cred);
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
pub mod session;
#[cfg(feature = "ssh-agent")]
pub mod ssh_agent;
#[cfg(feature = "wallet")]
pub mod wallet;
pub mod webauthn;
pub mod workflow;

mod meta;
mod newtypes;
pub use config::{AccountDerivation, Challenger, PassRuntimeConfig, RuntimeOverrides};
pub use meta::CredentialMeta;
pub use newtypes::{Account, AuthorityId, DeviceId, HashedUserId, InvalidHashedUserId};
pub use session::{SessionAuthorizer, SessionManager, SessionPolicy};
pub use workflow::{DeviceAuthenticator, EnrollmentDraft, PassAuthorizer, PassProfile};

use alloc::string::String;
use alloc::vec::Vec;
use codec::Encode;

use sube::extrinsic::{ChainContext, encode_extensions};
use sube::metadata::{ExtrinsicMeta, SignedExtensionMeta};
use sube::{DynValue, Error, ExtrinsicAssembler, Registry, Result};

/// Fixed-size challenge produced by pallet-pass challengers.
pub type Challenge = [u8; 32];

/// Generates a credential as a [`DynValue`] given the extrinsic context.
///
/// The extrinsic context is the blake2b-256 hash of the "inherited implication"
/// that pallet-pass uses to verify the credential. The returned [`DynValue`]
/// is serialized to SCALE by `scales` against the credential type from metadata.
pub trait CredentialProvider {
    async fn credential(&self, extrinsic_context: &[u8; 32]) -> Result<DynValue>;
}

/// Blake2b-256 — matches Substrate's `sp_core::hashing::blake2_256`.
///
/// Uses Blake2b (not Blake2s) with a 32-byte output to produce bit-identical
/// results to the runtime's hashing host function.
pub(crate) fn blake2b_256(data: &[u8]) -> [u8; 32] {
    use blake2::digest::{Update, VariableOutput};
    let mut hasher = blake2::Blake2bVar::new(32).expect("32 is a valid output size");
    hasher.update(data);
    let mut out = [0u8; 32];
    hasher
        .finalize_variable(&mut out)
        .expect("32 is a valid output size");
    out
}

/// Compute Kreivo's block challenger:
/// `blake2_256(block_hash(context) || binding)`.
///
/// The caller must resolve the context block to its hash. Hashing the encoded
/// block number here would be incorrect and would never match the runtime.
pub fn block_challenge(block_hash: &[u8; 32], binding: &[u8]) -> Challenge {
    let mut input = Vec::with_capacity(64);
    input.extend_from_slice(block_hash);
    input.extend_from_slice(binding);
    blake2b_256(&input)
}

/// Enrollment binds the challenge to the SCALE-encoded derived AccountId32.
pub fn enrollment_challenge(block_hash: &[u8; 32], account: Account) -> Challenge {
    block_challenge(block_hash, &account.0.encode())
}

/// V5 General extrinsic version prefix (bit 6 set).
const GENERAL_PREFIX: u8 = 0b01000000;
/// Extension identifier used by pallet-pass.
const PASS_AUTHENTICATE: &str = "PassAuthenticate";

/// pallet-pass authenticator that produces V5 "General" extrinsics.
///
/// Implements [`ExtrinsicAssembler`] by encoding the `PassAuthenticate`
/// transaction extension with a credential obtained from the [`CredentialProvider`].
/// All SCALE encoding is delegated to `scales` via the type registry.
pub struct PassAuthenticator<C> {
    account: Account,
    device_id: DeviceId,
    credential_provider: C,
}

impl<C> PassAuthenticator<C> {
    /// Construct a new authenticator.
    ///
    /// The newtype wrappers on `account` and `device_id` prevent argument-swap bugs.
    pub fn new(account: Account, device_id: DeviceId, credential_provider: C) -> Self {
        Self {
            account,
            device_id,
            credential_provider,
        }
    }
}

impl<C: CredentialProvider> ExtrinsicAssembler for PassAuthenticator<C> {
    type Account = [u8; 32];

    fn nonce_account(&self) -> Self::Account {
        self.account.0
    }

    fn authorization(&self) -> sube::AuthorizationSummary {
        sube::AuthorizationSummary {
            signing_account: self.device_id.0.to_vec(),
            nonce_account: self.account.0.to_vec(),
            scheme: Some("PassAuthenticate".into()),
        }
    }

    async fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> Result<Vec<u8>> {
        let (_before, _pass_ext, after) = locate_pass_authenticate(&meta.extensions)?;

        // Encode extensions AFTER PassAuthenticate to compute the inherited implication.
        let (after_extra, after_additional) = encode_extensions(after, registry, ctx, overrides)?;

        let version_byte = GENERAL_PREFIX | meta.version;
        let implication =
            inherited_implication(version_byte, encoded_call, &after_extra, &after_additional);

        // Ask the provider for the credential bound to this implication.
        let credential_value = self.credential_provider.credential(&implication).await?;

        // Build Option::Some(AuthenticateParams { device_id, credential }).
        let pass_value = DynValue::obj(&[(
            "Some",
            DynValue::obj(&[
                ("device_id", DynValue::from(self.device_id.0)),
                ("credential", credential_value),
            ]),
        )]);

        // Re-encode ALL extensions with the PassAuthenticate override in place.
        let mut all_overrides = Vec::from(overrides);
        all_overrides.push((PASS_AUTHENTICATE.into(), pass_value));
        let (all_extra, _) = encode_extensions(&meta.extensions, registry, ctx, &all_overrides)?;

        // Assemble the V5 General extrinsic: [ver, ext_ver, extras, call]
        let mut inner = Vec::with_capacity(2 + all_extra.len() + encoded_call.len());
        inner.push(version_byte);
        inner.push(0u8); // extension_version
        inner.extend_from_slice(&all_extra);
        inner.extend_from_slice(encoded_call);

        Ok(inner)
    }
}

/// Split the extension list at `PassAuthenticate`.
fn locate_pass_authenticate(
    exts: &[SignedExtensionMeta],
) -> Result<(
    &[SignedExtensionMeta],
    &SignedExtensionMeta,
    &[SignedExtensionMeta],
)> {
    let idx = exts
        .iter()
        .position(|e| e.identifier == PASS_AUTHENTICATE)
        .ok_or_else(|| {
            Error::MissingExtensionValue("PassAuthenticate not found in metadata".into())
        })?;
    let (before, rest) = exts.split_at(idx);
    let (pass_ext, after) = rest.split_first().expect("idx came from position()");
    Ok((before, pass_ext, after))
}

/// Compute the inherited implication that the signer binds to:
/// `blake2b_256(version_byte || encoded_call || after_extras || after_additionals)`.
pub fn inherited_implication(
    version_byte: u8,
    encoded_call: &[u8],
    after_extra: &[u8],
    after_additional: &[u8],
) -> [u8; 32] {
    let mut raw =
        Vec::with_capacity(1 + encoded_call.len() + after_extra.len() + after_additional.len());
    raw.push(version_byte);
    raw.extend_from_slice(encoded_call);
    raw.extend_from_slice(after_extra);
    raw.extend_from_slice(after_additional);
    blake2b_256(&raw)
}

#[cfg(test)]
mod blake2b_tests {
    use super::*;

    /// Known-answer test: Blake2b-256 of b"abc".
    /// Matches `sp_core::hashing::blake2_256(b"abc")`.
    #[test]
    fn blake2b_256_matches_sp_core_vector() {
        let expected: [u8; 32] = [
            0xbd, 0xdd, 0x81, 0x3c, 0x63, 0x42, 0x39, 0x72, 0x31, 0x71, 0xef, 0x3f, 0xee, 0x98,
            0x57, 0x9b, 0x94, 0x96, 0x4e, 0x3b, 0xb1, 0xcb, 0x3e, 0x42, 0x72, 0x62, 0xc8, 0xc0,
            0x68, 0xd5, 0x23, 0x19,
        ];
        assert_eq!(blake2b_256(b"abc"), expected);
    }

    #[test]
    fn blake2b_256_empty_input() {
        let expected: [u8; 32] = [
            0x0e, 0x57, 0x51, 0xc0, 0x26, 0xe5, 0x43, 0xb2, 0xe8, 0xab, 0x2e, 0xb0, 0x60, 0x99,
            0xda, 0xa1, 0xd1, 0xe5, 0xdf, 0x47, 0x77, 0x8f, 0x77, 0x87, 0xfa, 0xab, 0x45, 0xcd,
            0xf1, 0x2f, 0xe3, 0xa8,
        ];
        assert_eq!(blake2b_256(b""), expected);
    }

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
}
