//! WebAuthn credential provider for pallet-pass.
//!
//! The core of this module is the [`Authenticator`] trait, which abstracts
//! over the platform that actually performs the FIDO2 assertion. Concrete
//! backends (browser passkeys via `web-sys`, CTAP-HID via USB, software
//! authenticators via `passkey-authenticator`) live behind feature flags.
//!
//! The [`WebAuthnCredential`] struct wraps any [`Authenticator`] and
//! implements [`CredentialProvider`] for pallet-pass.
//!
//! # Example
//!
//! ```rust,ignore
//! use pass::webauthn::{Authenticator, AssertionResponse, WebAuthnCredential};
//! use pass::{AuthorityId, CredentialMeta, HashedUserId};
//!
//! let cred = WebAuthnCredential::new(
//!     CredentialMeta::new(
//!         HashedUserId(user_hash),
//!         AuthorityId(authority),
//!         current_block,
//!         "WebAuthn",
//!     ),
//!     my_authenticator,
//! );
//! ```

use alloc::string::String;
use alloc::vec::Vec;

use codec::Encode;

use crate::{block_challenge, CredentialMeta, CredentialProvider};
use sube::{DynValue, Result};

/// Errors a WebAuthn [`Authenticator`] backend may report.
#[derive(Debug)]
#[non_exhaustive]
pub enum AuthenticatorError {
    /// The user declined or canceled the assertion prompt.
    Canceled,
    /// No registered credential matched the request.
    NoCredential,
    /// Transport-level failure (USB disconnect, WebSocket drop, JS exception).
    Transport(String),
    /// Any other backend-specific failure.
    Other(String),
}

impl core::fmt::Display for AuthenticatorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Canceled => f.write_str("user canceled"),
            Self::NoCredential => f.write_str("no matching credential"),
            Self::Transport(m) => write!(f, "transport error: {m}"),
            Self::Other(m) => write!(f, "{m}"),
        }
    }
}

/// Raw assertion response from a WebAuthn authenticator.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
/// The challenge is a 32-byte blake2b-256 hash that the backend must
/// base64url-encode into the WebAuthn `client_data.challenge` field.
pub trait Authenticator {
    async fn assert(
        &self,
        challenge: &[u8; 32],
    ) -> core::result::Result<AssertionResponse, AuthenticatorError>;
}

/// WebAuthn credential provider for pallet-pass.
///
/// Wraps any [`Authenticator`] backend and produces a credential matching
/// pallet-pass's WebAuthn `Assertion` type:
///
/// ```text
/// { "WebAuthn": {
///     meta: { authority_id, user_id, context },
///     authenticator_data, client_data, signature
/// } }
/// ```
pub struct WebAuthnCredential<Cx, A> {
    meta: CredentialMeta<Cx>,
    authenticator: A,
}

impl<Cx, A> WebAuthnCredential<Cx, A>
where
    Cx: Encode + Into<DynValue> + Clone,
{
    pub fn new(meta: CredentialMeta<Cx>, authenticator: A) -> Self {
        Self { meta, authenticator }
    }

    /// Shortcut constructor without an explicit [`CredentialMeta`]. Uses
    /// `"WebAuthn"` as the composite enum variant name.
    pub fn with_parts(
        user_id: crate::HashedUserId,
        authority_id: crate::AuthorityId,
        context: Cx,
        authenticator: A,
    ) -> Self {
        Self::new(
            CredentialMeta::new(user_id, authority_id, context, "WebAuthn"),
            authenticator,
        )
    }
}

impl<Cx, A> CredentialProvider for WebAuthnCredential<Cx, A>
where
    Cx: Encode + Into<DynValue> + Clone,
    A: Authenticator,
{
    async fn credential(&self, extrinsic_context: &[u8; 32]) -> Result<DynValue> {
        let challenge = block_challenge(&self.meta.context, extrinsic_context);

        let resp = self
            .authenticator
            .assert(&challenge)
            .await
            .map_err(|e| sube::Error::Signing(alloc::format!("{e}")))?;

        let assertion = DynValue::obj(&[
            ("meta", self.meta.to_assertion_meta()),
            ("authenticator_data", DynValue::from(resp.authenticator_data)),
            ("client_data", DynValue::from(resp.client_data)),
            ("signature", DynValue::from(resp.signature)),
        ]);

        Ok(DynValue::obj(&[(self.meta.variant, assertion)]))
    }
}
