//! WebAuthn credential provider for pallet-pass.
//!
//! [`WebAuthnTransport`] abstracts platform create/get operations so desktop
//! USB, Windows WebAuthn, and deterministic virtual authenticators share the
//! same provider-neutral workflow.
//!
//! [`WebAuthnDevice`] turns any transport into a [`DeviceAuthenticator`]
//! for enrollment and for `pass::PassAuthenticator`.

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::workflow::{ChallengeRequest, DeviceAttestation, DeviceAuthenticator};
use crate::{DeviceId, blake2b_256};
use sube::{DynValue, Result};

/// Errors a [`WebAuthnTransport`] may report.
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
            // The runtime stores this key and verifies every later assertion
            // against it as a DER SubjectPublicKeyInfo.
            let (credential_id, public_key) = attested_p256_key(&authenticator_data)?;
            if credential_id != credential.raw_id {
                return Err(AuthenticatorError::Other(
                    "WebAuthn attested credential id does not match the credential".into(),
                ));
            }
            Ok(AttestationResponse {
                credential_id: credential.raw_id,
                authenticator_data,
                client_data: credential.response.client_data_json,
                public_key: public_key.to_vec(),
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

    /// DER `SubjectPublicKeyInfo` prefix of an uncompressed P-256 key
    /// (id-ecPublicKey, prime256v1), as `pallet-pass`'s WebAuthn verifier
    /// expects it.
    const P256_SPKI_PREFIX: [u8; 26] = [
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08,
        0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
    ];

    /// Extract the attested credential id and its ES256 (P-256) public key,
    /// DER-encoded, from WebAuthn authenticator data.
    pub(super) fn attested_p256_key(
        authenticator_data: &[u8],
    ) -> core::result::Result<(Vec<u8>, [u8; 91]), AuthenticatorError> {
        let invalid = |what: &str| AuthenticatorError::Other(format!("WebAuthn {what}"));
        // rpIdHash (32) | flags (1) | signCount (4) | aaguid (16) | len (2) | id | key
        const AT: u8 = 0x40;
        let flags = *authenticator_data
            .get(32)
            .ok_or_else(|| invalid("authenticator data is truncated"))?;
        if flags & AT == 0 {
            return Err(invalid("authenticator data has no attested credential"));
        }
        let rest = authenticator_data
            .get(53..)
            .ok_or_else(|| invalid("authenticator data is truncated"))?;
        let (len, rest) = rest
            .split_first_chunk::<2>()
            .ok_or_else(|| invalid("authenticator data is truncated"))?;
        let len = usize::from(u16::from_be_bytes(*len));
        if rest.len() < len {
            return Err(invalid("authenticator data is truncated"));
        }
        let (credential_id, cose) = rest.split_at(len);
        // The COSE key may be followed by extension data.
        let key = serde_cbor_2::Deserializer::from_slice(cose)
            .into_iter::<Value>()
            .next()
            .ok_or_else(|| invalid("credential has no public key"))?
            .map_err(|error| invalid(&format!("credential public key is invalid: {error}")))?;
        let Value::Map(key) = key else {
            return Err(invalid("credential public key is not a COSE key"));
        };
        let int = |label: i128| match key.get(&Value::Integer(label)) {
            Some(Value::Integer(value)) => Some(*value),
            _ => None,
        };
        let coordinate = |label: i128| match key.get(&Value::Integer(label)) {
            Some(Value::Bytes(bytes)) if bytes.len() == 32 => Some(bytes.as_slice()),
            _ => None,
        };
        // kty EC2 (2), alg ES256 (-7), crv P-256 (1)
        if int(1) != Some(2) || int(3) != Some(-7) || int(-1) != Some(1) {
            return Err(invalid("credential key is not an ES256 P-256 key"));
        }
        let (Some(x), Some(y)) = (coordinate(-2), coordinate(-3)) else {
            return Err(invalid("credential key has invalid coordinates"));
        };
        let mut der = [0u8; 91];
        der[..26].copy_from_slice(&P256_SPKI_PREFIX);
        der[26] = 0x04;
        der[27..59].copy_from_slice(x);
        der[59..].copy_from_slice(y);
        Ok((credential_id.to_vec(), der))
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

        /// Authenticator data attesting `credential_id` with an ES256 COSE key,
        /// followed by `trailing` (extension) bytes.
        fn auth_data_with_key(
            credential_id: &[u8],
            x: &[u8],
            y: &[u8],
            trailing: &[u8],
        ) -> Vec<u8> {
            use alloc::collections::BTreeMap;
            let mut key = BTreeMap::new();
            key.insert(Value::Integer(1), Value::Integer(2));
            key.insert(Value::Integer(3), Value::Integer(-7));
            key.insert(Value::Integer(-1), Value::Integer(1));
            key.insert(Value::Integer(-2), Value::Bytes(x.to_vec()));
            key.insert(Value::Integer(-3), Value::Bytes(y.to_vec()));
            let mut data = vec![7; 32];
            data.push(0x45); // UP | UV | AT
            data.extend([0, 0, 0, 1]);
            data.extend([0; 16]);
            data.extend((credential_id.len() as u16).to_be_bytes());
            data.extend(credential_id);
            data.extend(serde_cbor_2::to_vec(&Value::Map(key)).unwrap());
            data.extend(trailing);
            data
        }

        #[test]
        fn attested_key_is_der_encoded_like_openssl() {
            // `openssl ec -pubout -outform DER` of a P-256 key.
            let expected = hex::decode(
                "3059301306072a8648ce3d020106082a8648ce3d03010703420004\
                 c89152d85396f757b6d4e66e09916265daf72edfc2cd724cab48f0d1a1e5810a\
                 92d8eea8f84fa4a3767c239b918dd0409d1b90ef8f1a0ea6bbba3e8e1169ede0",
            )
            .unwrap();
            let (x, y) = expected[27..].split_at(32);
            let extensions = serde_cbor_2::to_vec(&Value::Text("ext".into())).unwrap();
            let data = auth_data_with_key(&[3, 4, 5], x, y, &extensions);
            let (credential_id, der) = attested_p256_key(&data).unwrap();
            assert_eq!(credential_id, [3, 4, 5]);
            assert_eq!(der.as_slice(), expected.as_slice());
        }

        #[test]
        fn attested_key_rejects_missing_or_foreign_keys() {
            let mut data = auth_data_with_key(&[1], &[1; 32], &[2; 32], &[]);
            assert!(attested_p256_key(&data[..60]).is_err());
            data[32] = 0x05; // AT cleared
            assert!(attested_p256_key(&data).is_err());
            let short_y = auth_data_with_key(&[1], &[1; 32], &[2; 31], &[]);
            assert!(attested_p256_key(&short_y).is_err());
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
        }
    }

    pub fn from_profile(transport: T, profile: WebAuthnProfile) -> Self {
        Self {
            transport,
            rp_id: profile.rp_id,
            origin: profile.origin,
            credential_id: RefCell::new(Some(profile.credential_id)),
        }
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
    const VARIANT: &'static str = "WebAuthn";

    async fn attest(&self, request: &ChallengeRequest) -> Result<DeviceAttestation> {
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
            variant: Self::VARIANT.into(),
            payload: DynValue::obj(&[
                ("meta", request.registration_meta(device_id)),
                (
                    "authenticator_data",
                    DynValue::from(response.authenticator_data),
                ),
                ("client_data", DynValue::from(response.client_data)),
                ("public_key", DynValue::from(response.public_key)),
            ]),
        })
    }

    async fn assert(&self, request: &ChallengeRequest) -> Result<DynValue> {
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
            Self::VARIANT,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::ChallengeRequest;
    use crate::{AuthorityId, HashedUserId};
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

    fn attestation_request() -> ChallengeRequest {
        ChallengeRequest {
            user_id: HashedUserId([1; 32]),
            authority_id: AuthorityId([3; 32]),
            context: 4,
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

        let request = ChallengeRequest {
            user_id: HashedUserId([1; 32]),
            authority_id: AuthorityId([3; 32]),
            context: 4,
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
