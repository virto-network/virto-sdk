//! Credential provider backed by [`libwallet`] signers.
//!
//! Works with any [`libwallet::Signer`] — software keys (Sr25519, Ed25519, Ecdsa),
//! hardware wallets (Ledger, Trezor), or derived keys from any vault.
//!
//! # Example
//!
//! ```rust,ignore
//! use pass::wallet::{SignatureType, WalletCredential};
//! use pass::{AuthorityId, HashedUserId};
//!
//! let cred = WalletCredential::new(
//!     CredentialMeta::new(
//!         HashedUserId(user_hash),
//!         AuthorityId(authority),
//!         current_block,
//!         current_block_hash,
//!         "SubstrateKey",
//!     ),
//!     SignatureType::Sr25519,
//!     &my_signer,
//! );
//! ```

use alloc::vec::Vec;
use codec::Encode;

use crate::workflow::{
    AssertionRequest, AttestationRequest, DeviceAttestation, DeviceAuthenticator,
};
use crate::{
    AuthorityId, Challenge, CredentialMeta, CredentialProvider, DeviceId, blake2b_256,
    block_challenge,
};
use sube::{DynValue, Error, Result};

/// Substrate `MultiSignature` variant.
///
/// Must be specified explicitly at credential construction. The libwallet
/// `Signer` trait does not reliably expose the underlying algorithm (a
/// `DerivedSigner` returns its derivation path from `account_id()`, not the
/// algorithm name), and guessing wrong produces credentials that fail
/// on-chain verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignatureType {
    Sr25519,
    Ed25519,
    Ecdsa,
}

impl SignatureType {
    fn variant_name(self) -> &'static str {
        match self {
            Self::Sr25519 => "Sr25519",
            Self::Ed25519 => "Ed25519",
            Self::Ecdsa => "Ecdsa",
        }
    }
}

/// Credential provider backed by a [`libwallet::Signer`].
///
/// Produces a `KeySignature`-shaped credential by:
/// 1. Computing a challenge from `(context, extrinsic_context)` via the
///    pallet-pass block challenger.
/// 2. SCALE-encoding `SignedMessage { context, challenge, authority_id }`.
/// 3. Signing the encoded message with the libwallet signer.
/// 4. Returning a `DynValue` that `scales` serializes to the runtime credential
///    type directly from metadata.
pub struct WalletCredential<'a, Cx, S: libwallet::Signer> {
    meta: CredentialMeta<Cx>,
    signature_type: SignatureType,
    signer: &'a S,
}

impl<'a, Cx, S: libwallet::Signer> WalletCredential<'a, Cx, S>
where
    Cx: Encode + Into<DynValue> + Clone,
{
    /// Create a credential provider from a libwallet signer.
    ///
    /// The signature type must be specified explicitly — see [`SignatureType`]
    /// for why.
    pub fn new(meta: CredentialMeta<Cx>, signature_type: SignatureType, signer: &'a S) -> Self {
        Self {
            meta,
            signature_type,
            signer,
        }
    }

    /// Shortcut constructor without an explicit [`CredentialMeta`]. Uses
    /// `"SubstrateKey"` as the composite enum variant name.
    pub fn with_parts(
        user_id: crate::HashedUserId,
        authority_id: AuthorityId,
        context: Cx,
        block_hash: [u8; 32],
        signature_type: SignatureType,
        signer: &'a S,
    ) -> Self {
        Self::new(
            CredentialMeta::new(user_id, authority_id, context, block_hash, "SubstrateKey"),
            signature_type,
            signer,
        )
    }
}

/// SCALE-encode `SignedMessage { context, challenge, authority_id }`.
fn encode_signed_message<Cx: Encode>(
    context: &Cx,
    challenge: &Challenge,
    authority_id: &AuthorityId,
) -> Vec<u8> {
    let mut out = Vec::new();
    context.encode_to(&mut out);
    challenge.encode_to(&mut out);
    authority_id.0.encode_to(&mut out);
    out
}

impl<Cx, S> CredentialProvider for WalletCredential<'_, Cx, S>
where
    Cx: Encode + Into<DynValue> + Clone,
    S: libwallet::Signer,
{
    async fn credential(&self, extrinsic_context: &[u8; 32]) -> Result<DynValue> {
        let challenge = block_challenge(&self.meta.block_hash, extrinsic_context);
        let message_bytes =
            encode_signed_message(&self.meta.context, &challenge, &self.meta.authority_id);

        let signature = self
            .signer
            .sign_msg(&message_bytes)
            .await
            .map_err(|e| Error::Signing(alloc::format!("{e}")))?;

        let signature_val = DynValue::obj(&[(
            self.signature_type.variant_name(),
            DynValue::from(signature.as_ref()),
        )]);
        let key_signature = DynValue::obj(&[
            ("user_id", DynValue::from(self.meta.user_id.0)),
            ("message", self.meta.to_signed_message(&challenge)),
            ("signature", signature_val),
        ]);

        Ok(DynValue::obj(&[(self.meta.variant, key_signature)]))
    }
}

/// Substrate-key device provider used for both registration and direct
/// authentication. The public key is explicit because libwallet's generic
/// signer trait intentionally does not require exporting it.
pub struct WalletDevice<'a, S: libwallet::Signer> {
    signer: &'a S,
    public_key: Vec<u8>,
    signature_type: SignatureType,
    attestation_variant: &'static str,
    credential_variant: &'static str,
}

impl<'a, S: libwallet::Signer> WalletDevice<'a, S> {
    pub fn new(signer: &'a S, public_key: impl AsRef<[u8]>, signature_type: SignatureType) -> Self {
        Self {
            signer,
            public_key: public_key.as_ref().to_vec(),
            signature_type,
            attestation_variant: "SubstrateKey",
            credential_variant: "SubstrateKey",
        }
    }

    pub fn with_variants(
        mut self,
        attestation_variant: &'static str,
        credential_variant: &'static str,
    ) -> Self {
        self.attestation_variant = attestation_variant;
        self.credential_variant = credential_variant;
        self
    }

    pub fn device_id(&self) -> DeviceId {
        DeviceId(blake2b_256(&self.public_key))
    }

    fn signature_value(&self, signature: impl AsRef<[u8]>) -> DynValue {
        DynValue::obj(&[(
            self.signature_type.variant_name(),
            DynValue::from(signature.as_ref()),
        )])
    }
}

impl<S: libwallet::Signer> DeviceAuthenticator for WalletDevice<'_, S> {
    async fn attest(&self, request: &AttestationRequest) -> Result<DeviceAttestation> {
        let message =
            encode_signed_message(&request.context, &request.challenge, &request.authority_id);
        let signature = self
            .signer
            .sign_msg(&message)
            .await
            .map_err(|error| Error::Signing(alloc::format!("{error}")))?;
        let device_id = self.device_id();
        Ok(DeviceAttestation {
            device_id,
            variant: self.attestation_variant.into(),
            payload: DynValue::obj(&[
                (
                    "meta",
                    DynValue::obj(&[
                        ("authority_id", DynValue::from(request.authority_id.0)),
                        ("device_id", DynValue::from(device_id.0)),
                        ("context", DynValue::from(request.context)),
                    ]),
                ),
                (
                    "public_key",
                    DynValue::obj(&[(
                        self.signature_type.variant_name(),
                        DynValue::from(self.public_key.clone()),
                    )]),
                ),
                ("signature", self.signature_value(signature)),
            ]),
        })
    }

    async fn assert(&self, request: &AssertionRequest) -> Result<DynValue> {
        let message =
            encode_signed_message(&request.context, &request.challenge, &request.authority_id);
        let signature = self
            .signer
            .sign_msg(&message)
            .await
            .map_err(|error| Error::Signing(alloc::format!("{error}")))?;
        Ok(DynValue::obj(&[(
            self.credential_variant,
            DynValue::obj(&[
                ("user_id", DynValue::from(request.user_id.0)),
                (
                    "message",
                    DynValue::obj(&[
                        ("context", DynValue::from(request.context)),
                        ("challenge", DynValue::from(request.challenge)),
                        ("authority_id", DynValue::from(request.authority_id.0)),
                    ]),
                ),
                ("signature", self.signature_value(signature)),
            ]),
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{AssertionRequest, AttestationRequest, DeviceAuthenticator};

    struct MockSigner;

    impl libwallet::Signer for MockSigner {
        type Signature = [u8; 64];

        fn account_id(&self) -> &str {
            "mock"
        }

        async fn sign_msg(
            &self,
            _: impl AsRef<[u8]>,
        ) -> core::result::Result<Self::Signature, libwallet::SigningError> {
            Ok([9; 64])
        }

        async fn verify(&self, _: impl AsRef<[u8]>, _: impl AsRef<[u8]>) -> bool {
            true
        }
    }

    #[test]
    fn signed_message_layout_is_68_bytes_for_u32() {
        let authority = AuthorityId([0u8; 32]);
        let msg = encode_signed_message(&0u32, &[0; 32], &authority);
        assert_eq!(msg.len(), 68); // 4 + 32 + 32
    }

    #[test]
    fn substrate_key_enrollment_and_assertion_use_one_device_identity() {
        let device = WalletDevice::new(&MockSigner, [7; 32], SignatureType::Sr25519);
        let attestation_request = AttestationRequest {
            user_id: crate::HashedUserId([1; 32]),
            pass_account: crate::Account([2; 32]),
            authority_id: crate::AuthorityId([3; 32]),
            context: 4,
            block_hash: [5; 32],
            challenge: [6; 32],
        };
        let attestation =
            futures_lite::future::block_on(device.attest(&attestation_request)).unwrap();
        assert_eq!(attestation.device_id, device.device_id());
        assert_eq!(attestation.variant, "SubstrateKey");

        let assertion_request = AssertionRequest {
            user_id: attestation_request.user_id,
            authority_id: attestation_request.authority_id,
            context: attestation_request.context,
            block_hash: attestation_request.block_hash,
            binding: [8; 32],
            challenge: [9; 32],
        };
        assert!(futures_lite::future::block_on(device.assert(&assertion_request)).is_ok());
    }
}
