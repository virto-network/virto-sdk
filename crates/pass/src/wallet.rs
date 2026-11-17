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

use crate::workflow::{ChallengeRequest, DeviceAttestation, DeviceAuthenticator};
use crate::{DeviceId, blake2b_256};
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

    /// Sign the request's `SignedMessage` as a `MultiSignature` value.
    async fn sign(&self, request: &ChallengeRequest) -> Result<DynValue> {
        let signature = self
            .signer
            .sign_msg(request.signed_message())
            .await
            .map_err(|error| Error::Signing(alloc::format!("{error}")))?;
        Ok(DynValue::obj(&[(
            self.signature_type.variant_name(),
            DynValue::from(signature.as_ref()),
        )]))
    }
}

impl<S: libwallet::Signer> DeviceAuthenticator for WalletDevice<'_, S> {
    const VARIANT: &'static str = "SubstrateKey";

    async fn attest(&self, request: &ChallengeRequest) -> Result<DeviceAttestation> {
        let device_id = self.device_id();
        Ok(DeviceAttestation {
            device_id,
            variant: Self::VARIANT.into(),
            // substrate-keys `KeyRegistration { public, message, signature }`.
            payload: DynValue::obj(&[
                ("public", DynValue::from(device_id.0)),
                ("message", request.message_value()),
                ("signature", self.sign(request).await?),
            ]),
        })
    }

    async fn assert(&self, request: &ChallengeRequest) -> Result<DynValue> {
        Ok(DynValue::obj(&[(
            Self::VARIANT,
            DynValue::obj(&[
                ("user_id", DynValue::from(request.user_id.0)),
                ("message", request.message_value()),
                ("signature", self.sign(request).await?),
            ]),
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn substrate_key_enrollment_and_assertion_use_one_device_identity() {
        let device = WalletDevice::new(&MockSigner, [7; 32], SignatureType::Sr25519);
        let attestation_request = ChallengeRequest {
            user_id: crate::HashedUserId([1; 32]),
            authority_id: crate::AuthorityId([3; 32]),
            context: 4,
            challenge: [6; 32],
        };
        let attestation =
            futures_lite::future::block_on(device.attest(&attestation_request)).unwrap();
        assert_eq!(attestation.device_id, device.device_id());
        assert_eq!(attestation.variant, "SubstrateKey");

        let assertion_request = ChallengeRequest {
            user_id: attestation_request.user_id,
            authority_id: attestation_request.authority_id,
            context: attestation_request.context,
            challenge: [9; 32],
        };
        assert!(futures_lite::future::block_on(device.assert(&assertion_request)).is_ok());
    }
}
