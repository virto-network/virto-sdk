//! Values of the substrate-keys authenticator, for a signature that was made
//! elsewhere.
//!
//! [`WalletDevice`](crate::wallet::WalletDevice) signs and builds these in
//! one step. Use the functions here when the key answers apart from the
//! code that builds the transaction: sign
//! [`ChallengeRequest::signed_message`] and pass the signature in.

use blake2::{Blake2b256, Digest};
use sube::DynValue;

use crate::DeviceId;
use crate::workflow::{ChallengeRequest, DeviceAttestation};

/// Variant of key devices in the runtime's attestation and credential enums.
pub const VARIANT: &str = "SubstrateKey";

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

/// The device id of a key: the public key itself for sr25519 and ed25519,
/// its blake2-256 hash for a 33-byte ECDSA key.
pub fn device_id(public_key: &[u8]) -> DeviceId {
    DeviceId(match <[u8; 32]>::try_from(public_key) {
        Ok(public) => public,
        Err(_) => Blake2b256::digest(public_key).into(),
    })
}

/// `signature` as a `MultiSignature` value.
fn signature_value(kind: SignatureType, signature: &[u8]) -> DynValue {
    DynValue::obj(&[(kind.variant_name(), DynValue::from(signature))])
}

/// The attestation that registers the key `public_key`, given its
/// `signature` over `request.signed_message()`.
pub fn attestation(
    request: &ChallengeRequest,
    public_key: &[u8],
    kind: SignatureType,
    signature: &[u8],
) -> DeviceAttestation {
    let device_id = device_id(public_key);
    DeviceAttestation {
        device_id,
        variant: VARIANT.into(),
        // substrate-keys `KeyRegistration { public, message, signature }`.
        payload: DynValue::obj(&[
            ("public", DynValue::from(device_id.0)),
            ("message", request.message_value()),
            ("signature", signature_value(kind, signature)),
        ]),
    }
}

/// The credential that answers `request`, given the key's `signature` over
/// `request.signed_message()`.
pub fn credential(request: &ChallengeRequest, kind: SignatureType, signature: &[u8]) -> DynValue {
    // substrate-keys `KeySignature { user_id, message, signature }`.
    DynValue::obj(&[(
        VARIANT,
        DynValue::obj(&[
            ("user_id", DynValue::from(request.user_id.0)),
            ("message", request.message_value()),
            ("signature", signature_value(kind, signature)),
        ]),
    )])
}
