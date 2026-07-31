//! WebAuthn credential provider for pallet-pass.
//!
//! [`WebAuthnTransport`] abstracts platform create/get operations so desktop
//! USB, Windows WebAuthn, and deterministic virtual authenticators share the
//! same provider-neutral workflow.
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
//!         current_block_hash,
//!         "WebAuthn",
//!     ),
//!     my_authenticator,
//! );
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use codec::Encode;

use crate::workflow::{
    AssertionRequest, AttestationRequest, DeviceAttestation, DeviceAuthenticator,
};
use crate::{CredentialMeta, CredentialProvider, DeviceId, blake2b_256, block_challenge};
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

/// Raw registration response. Private key material never leaves the
/// authenticator; profiles retain only RP/origin and credential id.
#[derive(Debug, Clone)]
pub struct AttestationResponse {
    pub credential_id: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub client_data: Vec<u8>,
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAuthnProfile {
    pub rp_id: String,
    pub origin: String,
    pub credential_id: Vec<u8>,
}

pub struct CreateRequest<'a> {
    pub rp_id: &'a str,
    pub origin: &'a str,
    pub user_id: &'a [u8; 32],
    pub challenge: &'a [u8; 32],
}

pub struct GetRequest<'a> {
    pub rp_id: &'a str,
    pub origin: &'a str,
    pub credential_id: &'a [u8],
    pub challenge: &'a [u8; 32],
}

/// Internal transport boundary implemented by USB CTAP, Windows WebAuthn, or
/// deterministic virtual authenticators in tests.
pub trait WebAuthnTransport {
    async fn create(
        &self,
        request: &CreateRequest<'_>,
    ) -> core::result::Result<AttestationResponse, AuthenticatorError>;

    async fn get(
        &self,
        request: &GetRequest<'_>,
    ) -> core::result::Result<AssertionResponse, AuthenticatorError>;
}

#[cfg(all(
    feature = "desktop-webauthn",
    any(target_os = "linux", target_os = "macos", windows)
))]
mod desktop {
    use super::{
        AssertionResponse, AttestationResponse, AuthenticatorError, CreateRequest, GetRequest,
        WebAuthnTransport,
    };
    use alloc::format;
    use alloc::string::ToString;
    use alloc::vec::Vec;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_cbor_2::Value;
    use webauthn_authenticator_rs::WebauthnAuthenticator;
    use webauthn_authenticator_rs::prelude::{
        CreationChallengeResponse, RequestChallengeResponse, Url, WebauthnCError,
    };

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    type PlatformBackend = webauthn_authenticator_rs::mozilla::MozillaAuthenticator;
    #[cfg(windows)]
    type PlatformBackend = webauthn_authenticator_rs::win10::Win10;

    /// Native desktop WebAuthn transport.
    ///
    /// Linux and macOS use the Mozilla USB HID backend. Windows uses the
    /// operating system WebAuthn API, which can reach platform and roaming
    /// authenticators without direct private-key access.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct DesktopWebAuthnTransport;

    impl WebAuthnTransport for DesktopWebAuthnTransport {
        async fn create(
            &self,
            request: &CreateRequest<'_>,
        ) -> core::result::Result<AttestationResponse, AuthenticatorError> {
            let origin = parse_origin(request.origin)?;
            let options = create_options(request)?;
            let mut backend = PlatformBackend::default();
            let credential = backend
                .do_registration(origin, options)
                .map_err(map_backend_error)?;
            let authenticator_data =
                attestation_authenticator_data(&credential.response.attestation_object)?;

            // pallet-pass calls this field `public_key`, but its WebAuthn
            // verifier uses the opaque credential id to select the registered
            // authenticator key.
            Ok(AttestationResponse {
                credential_id: credential.raw_id.clone(),
                authenticator_data,
                client_data: credential.response.client_data_json,
                public_key: credential.raw_id,
            })
        }

        async fn get(
            &self,
            request: &GetRequest<'_>,
        ) -> core::result::Result<AssertionResponse, AuthenticatorError> {
            let origin = parse_origin(request.origin)?;
            let options = get_options(request)?;
            let mut backend = PlatformBackend::default();
            let credential = backend
                .do_authentication(origin, options)
                .map_err(map_backend_error)?;
            if credential.raw_id != request.credential_id {
                return Err(AuthenticatorError::NoCredential);
            }

            Ok(AssertionResponse {
                authenticator_data: credential.response.authenticator_data,
                client_data: credential.response.client_data_json,
                signature: credential.response.signature,
            })
        }
    }

    fn parse_origin(origin: &str) -> core::result::Result<Url, AuthenticatorError> {
        Url::parse(origin)
            .map_err(|error| AuthenticatorError::Other(format!("invalid WebAuthn origin: {error}")))
    }

    fn create_options(
        request: &CreateRequest<'_>,
    ) -> core::result::Result<CreationChallengeResponse, AuthenticatorError> {
        serde_json::from_value(serde_json::json!({
            "publicKey": {
                "rp": {
                    "name": request.rp_id,
                    "id": request.rp_id,
                },
                "user": {
                    "id": URL_SAFE_NO_PAD.encode(request.user_id),
                    "name": hex::encode(request.user_id),
                    "displayName": hex::encode(request.user_id),
                },
                "challenge": URL_SAFE_NO_PAD.encode(request.challenge),
                "pubKeyCredParams": [
                    { "type": "public-key", "alg": -7 }
                ],
                "timeout": 60_000,
                "authenticatorSelection": {
                    "residentKey": "preferred",
                    "requireResidentKey": false,
                    "userVerification": "preferred"
                },
                "attestation": "none"
            }
        }))
        .map_err(|error| {
            AuthenticatorError::Other(format!("cannot construct WebAuthn create request: {error}"))
        })
    }

    fn get_options(
        request: &GetRequest<'_>,
    ) -> core::result::Result<RequestChallengeResponse, AuthenticatorError> {
        serde_json::from_value(serde_json::json!({
            "publicKey": {
                "challenge": URL_SAFE_NO_PAD.encode(request.challenge),
                "timeout": 60_000,
                "rpId": request.rp_id,
                "allowCredentials": [{
                    "type": "public-key",
                    "id": URL_SAFE_NO_PAD.encode(request.credential_id),
                    "transports": ["usb", "internal"]
                }],
                "userVerification": "preferred"
            }
        }))
        .map_err(|error| {
            AuthenticatorError::Other(format!("cannot construct WebAuthn get request: {error}"))
        })
    }

    fn attestation_authenticator_data(
        attestation_object: &[u8],
    ) -> core::result::Result<Vec<u8>, AuthenticatorError> {
        let Value::Map(entries) =
            serde_cbor_2::from_slice(attestation_object).map_err(|error| {
                AuthenticatorError::Other(format!("invalid WebAuthn attestation object: {error}"))
            })?
        else {
            return Err(AuthenticatorError::Other(
                "WebAuthn attestation object is not a CBOR map".into(),
            ));
        };
        entries
            .into_iter()
            .find_map(|(key, value)| match (key, value) {
                (Value::Text(key), Value::Bytes(bytes)) if key == "authData" => Some(bytes),
                _ => None,
            })
            .ok_or_else(|| {
                AuthenticatorError::Other(
                    "WebAuthn attestation object has no authenticator data".into(),
                )
            })
    }

    fn map_backend_error(error: WebauthnCError) -> AuthenticatorError {
        match error {
            WebauthnCError::Cancelled => AuthenticatorError::Canceled,
            WebauthnCError::InvalidAssertion
            | WebauthnCError::NoSelectedToken
            | WebauthnCError::NoHidDevices => AuthenticatorError::NoCredential,
            error => AuthenticatorError::Transport(error.to_string()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use alloc::vec;

        #[test]
        fn requests_preserve_rp_credential_and_exact_challenge() {
            let create = create_options(&CreateRequest {
                rp_id: "example.com",
                origin: "https://example.com",
                user_id: &[1; 32],
                challenge: &[2; 32],
            })
            .unwrap();
            assert_eq!(create.public_key.rp.id, "example.com");
            assert_eq!(create.public_key.user.id, [1; 32]);
            assert_eq!(create.public_key.challenge, [2; 32]);

            let get = get_options(&GetRequest {
                rp_id: "example.com",
                origin: "https://example.com",
                credential_id: &[3, 4, 5],
                challenge: &[6; 32],
            })
            .unwrap();
            assert_eq!(get.public_key.rp_id, "example.com");
            assert_eq!(get.public_key.challenge, [6; 32]);
            assert_eq!(get.public_key.allow_credentials[0].id, [3, 4, 5]);
        }

        #[test]
        fn attestation_parser_extracts_authenticator_data() {
            let object = serde_cbor_2::to_vec(&Value::Map(
                vec![
                    (Value::Text("fmt".into()), Value::Text("none".into())),
                    (Value::Text("authData".into()), Value::Bytes(vec![7; 37])),
                    (
                        Value::Text("attStmt".into()),
                        Value::Map(Default::default()),
                    ),
                ]
                .into_iter()
                .collect(),
            ))
            .unwrap();
            assert_eq!(
                attestation_authenticator_data(&object).unwrap(),
                vec![7; 37]
            );
        }

        #[test]
        fn passkey_shared_bytes_keep_challenge_exact() {
            let challenge: passkey::types::Bytes = vec![9; 32].into();
            assert_eq!(challenge.as_slice(), &[9; 32]);
        }
    }
}

#[cfg(all(
    feature = "desktop-webauthn",
    any(target_os = "linux", target_os = "macos", windows)
))]
pub use desktop::DesktopWebAuthnTransport;

/// Desktop WebAuthn provider independent of the concrete platform transport.
pub struct WebAuthnDevice<T> {
    transport: T,
    rp_id: String,
    origin: String,
    credential_id: RefCell<Option<Vec<u8>>>,
    attestation_variant: &'static str,
    credential_variant: &'static str,
}

impl<T> WebAuthnDevice<T> {
    pub fn for_enrollment(
        transport: T,
        rp_id: impl Into<String>,
        origin: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            rp_id: rp_id.into(),
            origin: origin.into(),
            credential_id: RefCell::new(None),
            attestation_variant: "WebAuthn",
            credential_variant: "WebAuthn",
        }
    }

    pub fn from_profile(transport: T, profile: WebAuthnProfile) -> Self {
        Self {
            transport,
            rp_id: profile.rp_id,
            origin: profile.origin,
            credential_id: RefCell::new(Some(profile.credential_id)),
            attestation_variant: "WebAuthn",
            credential_variant: "WebAuthn",
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

    pub fn profile(&self) -> Option<WebAuthnProfile> {
        Some(WebAuthnProfile {
            rp_id: self.rp_id.clone(),
            origin: self.origin.clone(),
            credential_id: self.credential_id.borrow().clone()?,
        })
    }
}

impl<T: WebAuthnTransport> DeviceAuthenticator for WebAuthnDevice<T> {
    async fn attest(&self, request: &AttestationRequest) -> Result<DeviceAttestation> {
        let response = self
            .transport
            .create(&CreateRequest {
                rp_id: &self.rp_id,
                origin: &self.origin,
                user_id: &request.user_id.0,
                challenge: &request.challenge,
            })
            .await
            .map_err(authenticator_error)?;
        *self.credential_id.borrow_mut() = Some(response.credential_id.clone());
        let device_id = DeviceId(blake2b_256(&response.credential_id));
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
                    "authenticator_data",
                    DynValue::from(response.authenticator_data),
                ),
                ("client_data", DynValue::from(response.client_data)),
                ("public_key", DynValue::from(response.public_key)),
            ]),
        })
    }

    async fn assert(&self, request: &AssertionRequest) -> Result<DynValue> {
        let credential_id =
            self.credential_id.borrow().clone().ok_or_else(|| {
                sube::Error::Signing("WebAuthn profile has no credential id".into())
            })?;
        let response = self
            .transport
            .get(&GetRequest {
                rp_id: &self.rp_id,
                origin: &self.origin,
                credential_id: &credential_id,
                challenge: &request.challenge,
            })
            .await
            .map_err(authenticator_error)?;
        Ok(DynValue::obj(&[(
            self.credential_variant,
            DynValue::obj(&[
                (
                    "meta",
                    DynValue::obj(&[
                        ("authority_id", DynValue::from(request.authority_id.0)),
                        ("user_id", DynValue::from(request.user_id.0)),
                        ("context", DynValue::from(request.context)),
                    ]),
                ),
                (
                    "authenticator_data",
                    DynValue::from(response.authenticator_data),
                ),
                ("client_data", DynValue::from(response.client_data)),
                ("signature", DynValue::from(response.signature)),
            ]),
        )]))
    }
}

fn authenticator_error(error: AuthenticatorError) -> sube::Error {
    sube::Error::Signing(alloc::format!("{error}"))
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
        Self {
            meta,
            authenticator,
        }
    }

    /// Shortcut constructor without an explicit [`CredentialMeta`]. Uses
    /// `"WebAuthn"` as the composite enum variant name.
    pub fn with_parts(
        user_id: crate::HashedUserId,
        authority_id: crate::AuthorityId,
        context: Cx,
        block_hash: [u8; 32],
        authenticator: A,
    ) -> Self {
        Self::new(
            CredentialMeta::new(user_id, authority_id, context, block_hash, "WebAuthn"),
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
        let challenge = block_challenge(&self.meta.block_hash, extrinsic_context);

        let resp = self
            .authenticator
            .assert(&challenge)
            .await
            .map_err(|e| sube::Error::Signing(alloc::format!("{e}")))?;

        let assertion = DynValue::obj(&[
            ("meta", self.meta.to_assertion_meta()),
            (
                "authenticator_data",
                DynValue::from(resp.authenticator_data),
            ),
            ("client_data", DynValue::from(resp.client_data)),
            ("signature", DynValue::from(resp.signature)),
        ]);

        Ok(DynValue::obj(&[(self.meta.variant, assertion)]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{AssertionRequest, AttestationRequest};
    use crate::{Account, AuthorityId, HashedUserId};
    use alloc::{string::ToString, vec};

    struct VirtualAuthenticator {
        canceled: bool,
    }

    impl WebAuthnTransport for VirtualAuthenticator {
        async fn create(
            &self,
            _: &CreateRequest<'_>,
        ) -> core::result::Result<AttestationResponse, AuthenticatorError> {
            if self.canceled {
                return Err(AuthenticatorError::Canceled);
            }
            Ok(AttestationResponse {
                credential_id: vec![1, 2, 3],
                authenticator_data: vec![4; 37],
                client_data: br#"{"type":"webauthn.create"}"#.to_vec(),
                public_key: vec![5; 91],
            })
        }

        async fn get(
            &self,
            _: &GetRequest<'_>,
        ) -> core::result::Result<AssertionResponse, AuthenticatorError> {
            if self.canceled {
                return Err(AuthenticatorError::Canceled);
            }
            Ok(AssertionResponse {
                authenticator_data: vec![6; 37],
                client_data: br#"{"type":"webauthn.get"}"#.to_vec(),
                signature: vec![7; 64],
            })
        }
    }

    fn attestation_request() -> AttestationRequest {
        AttestationRequest {
            user_id: HashedUserId([1; 32]),
            pass_account: Account([2; 32]),
            authority_id: AuthorityId([3; 32]),
            context: 4,
            block_hash: [5; 32],
            challenge: [6; 32],
        }
    }

    #[test]
    fn virtual_create_and_get_keep_only_profile_identifiers() {
        let device = WebAuthnDevice::for_enrollment(
            VirtualAuthenticator { canceled: false },
            "example.com",
            "https://example.com",
        );
        let attestation =
            futures_lite::future::block_on(device.attest(&attestation_request())).unwrap();
        assert_eq!(attestation.variant, "WebAuthn");
        let profile = device.profile().unwrap();
        assert_eq!(profile.credential_id, vec![1, 2, 3]);

        let request = AssertionRequest {
            user_id: HashedUserId([1; 32]),
            authority_id: AuthorityId([3; 32]),
            context: 4,
            block_hash: [5; 32],
            binding: [8; 32],
            challenge: [9; 32],
        };
        assert!(futures_lite::future::block_on(device.assert(&request)).is_ok());
    }

    #[test]
    fn canceled_prompt_is_reported_without_fallback() {
        let device = WebAuthnDevice::for_enrollment(
            VirtualAuthenticator { canceled: true },
            "example.com",
            "https://example.com",
        );
        let error =
            futures_lite::future::block_on(device.attest(&attestation_request())).unwrap_err();
        assert!(error.to_string().contains("user canceled"));
    }
}
