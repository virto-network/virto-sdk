#![no_std]
//! pallet-pass authenticator for sube.
//!
//! Provides [`PassAuthenticator`] which implements sube's [`ExtrinsicAssembler`]
//! to produce V5 "General" extrinsics authenticated via the `PassAuthenticate`
//! transaction extension (as used by the Kreivo blockchain).
//!
//! # Usage
//!
//! ```rust,ignore
//! use pass::{PassAuthenticator, CredentialProvider};
//!
//! struct MyWebAuthn { /* ... */ }
//! impl CredentialProvider for MyWebAuthn {
//!     async fn credential(&self, ctx: &[u8; 32]) -> sube::Result<sube::DynValue> {
//!         // return DynValue representation of the credential
//!         // (scales serializes it against the runtime type from metadata)
//!         todo!()
//!     }
//! }
//!
//! let auth = PassAuthenticator::new(account, device_id, MyWebAuthn { /* ... */ });
//! chain.call("some_pallet/some_call")
//!     .body_text("(...)")
//!     .signer(auth)
//!     .await?;
//! ```

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;

use blake2::digest::Digest;
use blake2::Blake2s256;

use sube::extrinsic::{encode_extensions, ChainContext};
use sube::metadata::ExtrinsicMeta;
use sube::{DynValue, Error, ExtrinsicAssembler, Registry, Result};

/// Device identifier — 32 bytes, matches `fc_traits_authn::DeviceId`.
pub type DeviceId = [u8; 32];

/// Generates a credential as a [`DynValue`] given the extrinsic context.
///
/// The extrinsic context is the blake2-256 hash of the "inherited implication"
/// that pallet-pass uses to verify the credential. The returned [`DynValue`]
/// is serialized to SCALE by `scales` against the credential type from metadata.
///
/// Concrete implementations handle the actual credential generation
/// (WebAuthn, SubstrateKey, mock, etc.).
pub trait CredentialProvider {
    fn credential(&self, extrinsic_context: &[u8; 32]) -> impl Future<Output = Result<DynValue>>;
}

/// pallet-pass authenticator that produces V5 "General" extrinsics.
///
/// Implements [`ExtrinsicAssembler`] by encoding the `PassAuthenticate`
/// transaction extension with a credential obtained from the [`CredentialProvider`].
/// All SCALE encoding is delegated to `scales` via the type registry — no manual
/// byte construction.
pub struct PassAuthenticator<C> {
    /// Derived pass account address (for nonce lookup).
    account: [u8; 32],
    /// Registered device identifier.
    device_id: DeviceId,
    /// Credential generator.
    credential_provider: C,
}

impl<C> PassAuthenticator<C> {
    pub fn new(account: [u8; 32], device_id: DeviceId, credential_provider: C) -> Self {
        Self {
            account,
            device_id,
            credential_provider,
        }
    }
}

fn blake2_256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2s256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// V5 General extrinsic version prefix (bit 6 set).
const GENERAL_PREFIX: u8 = 0b01000000;

/// Extension identifier used by pallet-pass.
const PASS_AUTHENTICATE: &str = "PassAuthenticate";

impl<C: CredentialProvider> ExtrinsicAssembler for PassAuthenticator<C> {
    type Account = [u8; 32];

    fn account(&self) -> Self::Account {
        self.account
    }

    async fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> Result<Vec<u8>> {
        let exts = &meta.extensions;

        // Find PassAuthenticate extension index in metadata
        let pass_idx = exts
            .iter()
            .position(|e| e.identifier == PASS_AUTHENTICATE)
            .ok_or_else(|| {
                Error::MissingExtensionValue("PassAuthenticate not found in metadata".into())
            })?;

        let after = &exts[pass_idx + 1..];

        // Encode extensions AFTER PassAuthenticate to compute the inherited implication
        let (after_extra, after_additional) = encode_extensions(after, registry, ctx, overrides)?;

        // inherited implication = blake2_256([version_byte, call, after_extras, after_additionals])
        let version_byte = GENERAL_PREFIX | meta.version;
        let mut raw_context = Vec::new();
        raw_context.push(version_byte);
        raw_context.extend_from_slice(encoded_call);
        raw_context.extend_from_slice(&after_extra);
        raw_context.extend_from_slice(&after_additional);
        let inherited_implication = blake2_256(&raw_context);

        // Get credential DynValue from the provider
        let credential_value = self
            .credential_provider
            .credential(&inherited_implication)
            .await?;

        // Build PassAuthenticate value: Option<AuthenticateParams> = Some({ device_id, credential })
        let device_id_hex = alloc::format!("0x{}", hex::encode(self.device_id));
        let params = DynValue::obj(&[
            ("device_id", DynValue::from(device_id_hex)),
            ("credential", credential_value),
        ]);
        let pass_value = DynValue::obj(&[("Some", params)]);

        // Add PassAuthenticate override and encode ALL extensions through scales
        let mut all_overrides = Vec::from(overrides);
        all_overrides.push((PASS_AUTHENTICATE.into(), pass_value));
        let (all_extra, _all_additional) = encode_extensions(exts, registry, ctx, &all_overrides)?;

        // Assemble V5 General extrinsic:
        // [version_byte, extension_version(0u8), all_extras, encoded_call]
        let mut inner = Vec::new();
        inner.push(version_byte);
        inner.push(0u8); // extension_version
        inner.extend_from_slice(&all_extra);
        inner.extend_from_slice(encoded_call);

        Ok(inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockCredential(DynValue);

    impl CredentialProvider for MockCredential {
        fn credential(&self, _ctx: &[u8; 32]) -> impl Future<Output = Result<DynValue>> {
            let val = self.0.clone();
            async move { Ok(val) }
        }
    }

    #[test]
    fn pass_authenticator_created() {
        let account = [1u8; 32];
        let device_id = [2u8; 32];
        let cred = MockCredential(DynValue::Null);
        let auth = PassAuthenticator::new(account, device_id, cred);
        assert_eq!(ExtrinsicAssembler::account(&auth), [1u8; 32]);
    }
}
