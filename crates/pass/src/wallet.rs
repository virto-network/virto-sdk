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

use crate::DeviceId;
use crate::key;
pub use crate::key::SignatureType;
use crate::workflow::{ChallengeRequest, DeviceAttestation, DeviceAuthenticator};
use sube::{DynValue, Error, Result};

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
    /// uses as its device id; see [`key::device_id`].
    pub fn device_id(&self) -> DeviceId {
        key::device_id(&self.public_key)
    }

    /// Sign the request's `SignedMessage`.
    async fn sign(&self, request: &ChallengeRequest) -> Result<S::Signature> {
        self.signer
            .sign_msg(request.signed_message())
            .await
            .map_err(|error| Error::Signing(alloc::format!("{error}")))
    }
}

impl<S: libwallet::Signer> DeviceAuthenticator for WalletDevice<'_, S> {
    fn variant(&self) -> &'static str {
        key::VARIANT
    }

    async fn attest(&self, request: &ChallengeRequest) -> Result<DeviceAttestation> {
        let signature = self.sign(request).await?;
        Ok(key::attestation(
            request,
            &self.public_key,
            self.signature_type,
            signature.as_ref(),
        ))
    }

    async fn assert(&self, request: &ChallengeRequest) -> Result<DynValue> {
        let signature = self.sign(request).await?;
        Ok(key::credential(
            request,
            self.signature_type,
            signature.as_ref(),
        ))
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
        // substrate-keys `KeySignature { user_id, message, signature }`.
        assert_eq!(
            futures_lite::future::block_on(device.assert(&assertion_request)).unwrap(),
            DynValue::obj(&[(
                "SubstrateKey",
                DynValue::obj(&[
                    ("user_id", DynValue::from([1u8; 32])),
                    (
                        "message",
                        DynValue::obj(&[
                            ("context", DynValue::from(4u32)),
                            ("challenge", DynValue::from([9u8; 32])),
                            ("authority_id", DynValue::from([3u8; 32])),
                        ])
                    ),
                    (
                        "signature",
                        DynValue::obj(&[("Sr25519", DynValue::from([9u8; 64].as_slice()))])
                    ),
                ])
            )])
        );
    }
}
