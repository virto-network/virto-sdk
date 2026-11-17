//! Device provider backed by [`libwallet`] signers.
//!
//! Works with any [`libwallet::Signer`] — software keys (Sr25519, Ed25519, Ecdsa),
//! hardware wallets (Ledger, Trezor), or derived keys from any vault.
//!
//! # Example
//!
//! ```rust,ignore
//! use pass::wallet::{SignatureType, WalletDevice};
//!
//! // Registers through `pass::workflow::prepare_enrollment` and authenticates
//! // through `pass::PassAuthenticator`.
//! let device = WalletDevice::new(&signer, signer.public(), SignatureType::Sr25519);
//! let device_id = device.device_id();
//! ```

use alloc::vec::Vec;
use codec::Encode;

use crate::workflow::{
    AssertionRequest, AttestationRequest, DeviceAttestation, DeviceAuthenticator,
};
use crate::{AuthorityId, Challenge, DeviceId, blake2b_256};
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

/// Substrate-key device provider used for both registration and direct
/// authentication. The public key is explicit because libwallet's generic
/// signer trait intentionally does not require exporting it.
pub struct WalletDevice<'a, S: libwallet::Signer> {
    signer: &'a S,
    public_key: Vec<u8>,
    signature_type: SignatureType,
}

impl<'a, S: libwallet::Signer> WalletDevice<'a, S> {
    pub fn new(signer: &'a S, public_key: impl AsRef<[u8]>, signature_type: SignatureType) -> Self {
        Self {
            signer,
            public_key: public_key.as_ref().to_vec(),
            signature_type,
        }
    }

    /// The device's `AccountId32`, which the substrate-keys authenticator
    /// uses as its device id: the public key itself for sr25519/ed25519, its
    /// blake2-256 hash for a 33-byte ECDSA key.
    pub fn device_id(&self) -> DeviceId {
        DeviceId(match <[u8; 32]>::try_from(self.public_key.as_slice()) {
            Ok(public) => public,
            Err(_) => blake2b_256(&self.public_key),
        })
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
            variant: "SubstrateKey".into(),
            // substrate-keys `KeyRegistration { public, message, signature }`.
            payload: DynValue::obj(&[
                ("public", DynValue::from(device_id.0)),
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
            "SubstrateKey",
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
            authority_id: crate::AuthorityId([3; 32]),
            context: 4,
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
            challenge: [9; 32],
        };
        assert!(futures_lite::future::block_on(device.assert(&assertion_request)).is_ok());
    }
}
