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
//!         "SubstrateKey",
//!     ),
//!     SignatureType::Sr25519,
//!     &my_signer,
//! );
//! ```

use alloc::vec::Vec;
use codec::Encode;

use crate::{block_challenge, AuthorityId, Challenge, CredentialMeta, CredentialProvider};
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
        signature_type: SignatureType,
        signer: &'a S,
    ) -> Self {
        Self::new(
            CredentialMeta::new(user_id, authority_id, context, "SubstrateKey"),
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
        let challenge = block_challenge(&self.meta.context, extrinsic_context);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_message_layout_is_68_bytes_for_u32() {
        let authority = AuthorityId([0u8; 32]);
        let msg = encode_signed_message(&0u32, &[0; 32], &authority);
        assert_eq!(msg.len(), 68); // 4 + 32 + 32
    }
}
