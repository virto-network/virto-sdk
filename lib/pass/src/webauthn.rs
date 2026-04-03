//! WebAuthn credential provider for pallet-pass.
//!
//! Supports three authenticator backends (feature-gated):
//! - **`webauthn-web`**: Browser passkeys via `web-sys` (WASM)
//! - **`webauthn-soft`**: Software authenticator via `passkey-authenticator` (1Password)
//! - **`webauthn-ctap`**: Hardware tokens via USB+CTAP (YubiKey, SoloKey, etc.)
//!
//! All backends implement the [`Authenticator`] trait. The [`WebAuthnCredential`]
//! struct wraps any backend and implements [`CredentialProvider`].
//!
//! # Example
//!
//! ```rust,ignore
//! use pass::webauthn::{WebAuthnCredential, Authenticator, AssertionResponse};
//!
//! // Use any Authenticator implementation
//! let cred = WebAuthnCredential::new(
//!     hashed_user_id,
//!     authority_id,
//!     current_block,
//!     my_authenticator,
//! );
//! let auth = PassAuthenticator::new(account, device_id, cred);
//! ```

use alloc::vec::Vec;

use crate::{block_challenge, AuthorityId, CredentialProvider, HashedUserId};
use sube::{DynValue, Result};

/// Raw assertion response from a WebAuthn authenticator.
pub struct AssertionResponse {
    /// Authenticator data (RP ID hash + flags + counter).
    pub authenticator_data: Vec<u8>,
    /// Client data JSON (contains type, challenge, origin).
    pub client_data: Vec<u8>,
    /// ECDSA signature over `sha256(authenticator_data || sha256(client_data))`.
    pub signature: Vec<u8>,
}

/// Abstraction over a WebAuthn authenticator.
///
/// Backends implement this to perform a FIDO2 assertion (authentication).
/// The challenge is a 32-byte blake2-256 hash that gets base64url-encoded
/// into the WebAuthn `client_data.challenge` field.
pub trait Authenticator {
    /// Perform a WebAuthn assertion with the given 32-byte challenge.
    ///
    /// Returns the raw assertion response containing authenticator data,
    /// client data JSON, and signature bytes.
    async fn assert(&self, challenge: &[u8; 32]) -> Result<AssertionResponse>;
}

/// WebAuthn credential provider for pallet-pass.
///
/// Wraps any [`Authenticator`] backend and produces a JSON credential
/// matching pallet-pass's WebAuthn assertion structure:
///
/// ```text
/// { "WebAuthn": { meta: { authority_id, user_id, context }, authenticator_data, client_data, signature } }
/// ```
pub struct WebAuthnCredential<A> {
    hashed_user_id: HashedUserId,
    authority_id: AuthorityId,
    context: u32,
    authenticator: A,
    /// Variant name in the composite credential enum.
    variant: &'static str,
}

impl<A> WebAuthnCredential<A> {
    pub fn new(
        hashed_user_id: HashedUserId,
        authority_id: AuthorityId,
        context: u32,
        authenticator: A,
    ) -> Self {
        Self {
            hashed_user_id,
            authority_id,
            context,
            authenticator,
            variant: "WebAuthn",
        }
    }

    /// Override the variant name in the composite credential enum.
    pub fn variant(mut self, name: &'static str) -> Self {
        self.variant = name;
        self
    }
}

impl<A: Authenticator> CredentialProvider for WebAuthnCredential<A> {
    async fn credential(&self, extrinsic_context: &[u8; 32]) -> Result<DynValue> {
        let challenge = block_challenge(self.context, extrinsic_context);

        let resp = self.authenticator.assert(&challenge).await?;

        let authority_hex = alloc::format!("0x{}", hex::encode(self.authority_id));
        let user_id_hex = alloc::format!("0x{}", hex::encode(self.hashed_user_id));
        let auth_data_hex = alloc::format!("0x{}", hex::encode(&resp.authenticator_data));
        let client_data_hex = alloc::format!("0x{}", hex::encode(&resp.client_data));
        let signature_hex = alloc::format!("0x{}", hex::encode(&resp.signature));

        let meta = DynValue::obj(&[
            ("authority_id", DynValue::from(authority_hex)),
            ("user_id", DynValue::from(user_id_hex)),
            ("context", DynValue::from(self.context)),
        ]);
        let assertion = DynValue::obj(&[
            ("meta", meta),
            ("authenticator_data", DynValue::from(auth_data_hex)),
            ("client_data", DynValue::from(client_data_hex)),
            ("signature", DynValue::from(signature_hex)),
        ]);

        Ok(DynValue::obj(&[(self.variant, assertion)]))
    }
}
