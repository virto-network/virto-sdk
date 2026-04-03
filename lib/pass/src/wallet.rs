//! Credential provider backed by [`libwallet`] signers.
//!
//! Works with any [`libwallet::Signer`] — software keys (Sr25519, Ed25519, Ecdsa),
//! hardware wallets (Ledger, Trezor), or derived keys from any vault.
//!
//! # Example
//!
//! ```rust,ignore
//! use pass::wallet::WalletCredential;
//!
//! let cred = WalletCredential::new(
//!     hashed_user_id,
//!     authority_id,
//!     current_block,
//!     &my_signer,   // any libwallet::Signer
//! );
//! let auth = PassAuthenticator::new(account, device_id, cred);
//! ```

use alloc::vec::Vec;

use codec::Encode;

use crate::{block_challenge, AuthorityId, Challenge, CredentialProvider, HashedUserId};
use sube::{DynValue, Error, Result};

/// Substrate `MultiSignature` variant name.
#[derive(Debug, Clone, Copy)]
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

    /// Infer from a [`libwallet::Signer::account_id`] string.
    pub fn from_account_id(id: &str) -> Self {
        if id.contains("ed25519") {
            Self::Ed25519
        } else if id.contains("secp256k1") {
            Self::Ecdsa
        } else {
            Self::Sr25519
        }
    }
}

/// Credential provider backed by a [`libwallet::Signer`].
///
/// Produces a `KeySignature`-shaped credential by:
/// 1. Computing a challenge from `(context, extrinsic_context)` via block challenger
/// 2. SCALE-encoding the signed message `(context, challenge, authority_id)`
/// 3. Signing with the wallet signer
/// 4. Returning DynValue for scales serialization against the runtime type
pub struct WalletCredential<'a, S: libwallet::Signer> {
    hashed_user_id: HashedUserId,
    authority_id: AuthorityId,
    context: u32,
    signature_type: SignatureType,
    signer: &'a S,
    /// Variant name in the composite credential enum.
    variant: &'static str,
}

impl<'a, S: libwallet::Signer> WalletCredential<'a, S> {
    /// Create a credential provider from a libwallet signer.
    ///
    /// The [`SignatureType`] is inferred from [`Signer::account_id`](libwallet::Signer::account_id).
    /// For derived signers where `account_id` is a derivation path, this defaults
    /// to `Sr25519` — use [`.signature_type()`] to override.
    pub fn new(
        hashed_user_id: HashedUserId,
        authority_id: AuthorityId,
        context: u32,
        signer: &'a S,
    ) -> Self {
        let signature_type = SignatureType::from_account_id(signer.account_id());
        Self {
            hashed_user_id,
            authority_id,
            context,
            signature_type,
            signer,
            variant: "SubstrateKey",
        }
    }

    /// Override the signature algorithm.
    pub fn signature_type(mut self, ty: SignatureType) -> Self {
        self.signature_type = ty;
        self
    }

    /// Override the variant name in the composite credential enum.
    ///
    /// Defaults to `"SubstrateKey"`. Change this if the runtime's
    /// `composite_authenticators!` macro uses a different name.
    pub fn variant(mut self, name: &'static str) -> Self {
        self.variant = name;
        self
    }
}

/// SCALE-encode the signed message: `(context: u32, challenge: [u8;32], authority_id: [u8;32])`
fn encode_message(context: u32, challenge: &Challenge, authority_id: &AuthorityId) -> Vec<u8> {
    let mut out = Vec::new();
    context.encode_to(&mut out);
    challenge.encode_to(&mut out);
    authority_id.encode_to(&mut out);
    out
}

impl<S: libwallet::Signer> CredentialProvider for WalletCredential<'_, S> {
    async fn credential(&self, extrinsic_context: &[u8; 32]) -> Result<DynValue> {
        let challenge = block_challenge(self.context, extrinsic_context);
        let message_bytes = encode_message(self.context, &challenge, &self.authority_id);

        let signature = self
            .signer
            .sign_msg(&message_bytes)
            .await
            .map_err(|e| Error::Encode(alloc::format!("signing failed: {e}")))?;

        let sig_variant = self.signature_type.variant_name();
        let signature_hex = alloc::format!("0x{}", hex::encode(signature.as_ref()));
        let challenge_hex = alloc::format!("0x{}", hex::encode(challenge));
        let authority_hex = alloc::format!("0x{}", hex::encode(self.authority_id));
        let user_id_hex = alloc::format!("0x{}", hex::encode(self.hashed_user_id));

        let message = DynValue::obj(&[
            ("context", DynValue::from(self.context)),
            ("challenge", DynValue::from(challenge_hex)),
            ("authority_id", DynValue::from(authority_hex)),
        ]);
        let signature_val =
            DynValue::obj(&[(sig_variant, DynValue::from(signature_hex))]);
        let key_signature = DynValue::obj(&[
            ("user_id", DynValue::from(user_id_hex)),
            ("message", message),
            ("signature", signature_val),
        ]);

        // Wrap in the composite authenticator variant
        Ok(DynValue::obj(&[(self.variant, key_signature)]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_challenge_deterministic() {
        let ctx = 42u32;
        let xtc = [0xab; 32];
        let c1 = block_challenge(ctx, &xtc);
        let c2 = block_challenge(ctx, &xtc);
        assert_eq!(c1, c2);
        assert_ne!(c1, [0u8; 32]);
    }

    #[test]
    fn block_challenge_varies_with_context() {
        let xtc = [0xab; 32];
        assert_ne!(block_challenge(1, &xtc), block_challenge(2, &xtc));
    }

    #[test]
    fn block_challenge_varies_with_xtc() {
        assert_ne!(
            block_challenge(1, &[0x01; 32]),
            block_challenge(1, &[0x02; 32])
        );
    }

    #[test]
    fn encode_message_is_68_bytes() {
        let msg = encode_message(0, &[0; 32], &[0; 32]);
        assert_eq!(msg.len(), 68); // 4 + 32 + 32
    }
}
