//! Provider-neutral enrollment and device-management workflows.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};

use codec::Encode;

use sube::{DynValue, Error, Metadata, PreparedCall, Result};

use crate::config::{PassRuntimeConfig, body};
use crate::{
    Account, AuthorityId, Challenge, DeviceFilter, DeviceId, HashedUserId, enrollment_challenge,
};

/// What a device provider is asked to sign, for enrollment and for
/// authentication alike: a challenge bound to the `context` block, for a user
/// of the runtime's authority.
#[derive(Clone, Debug)]
pub struct ChallengeRequest {
    pub user_id: HashedUserId,
    pub authority_id: AuthorityId,
    pub context: u32,
    pub challenge: Challenge,
}

impl ChallengeRequest {
    /// Challenge a device to register for `pass_account` at `context`, whose
    /// block hash is `block_hash`.
    pub fn enrollment(
        user_id: HashedUserId,
        pass_account: Account,
        authority_id: AuthorityId,
        context: u32,
        block_hash: &[u8; 32],
    ) -> Self {
        Self {
            user_id,
            authority_id,
            context,
            challenge: enrollment_challenge(block_hash, pass_account),
        }
    }

    /// The SCALE-encoded `SignedMessage { context, challenge, authority_id }`
    /// that key-based devices sign.
    pub fn signed_message(&self) -> Vec<u8> {
        (self.context, self.challenge, self.authority_id.0).encode()
    }

    /// `SignedMessage { context, challenge, authority_id }` as a runtime value.
    pub fn message_value(&self) -> DynValue {
        DynValue::obj(&[
            ("context", DynValue::from(self.context)),
            ("challenge", DynValue::from(self.challenge)),
            ("authority_id", DynValue::from(self.authority_id.0)),
        ])
    }

    /// Registration metadata `{ authority_id, device_id, context }`.
    pub fn registration_meta(&self, device_id: DeviceId) -> DynValue {
        DynValue::obj(&[
            ("authority_id", DynValue::from(self.authority_id.0)),
            ("device_id", DynValue::from(device_id.0)),
            ("context", DynValue::from(self.context)),
        ])
    }
}

/// Provider-neutral registration output.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceAttestation {
    pub device_id: DeviceId,
    pub variant: String,
    /// Payload inside the runtime's attestation enum variant.
    pub payload: DynValue,
}

impl DeviceAttestation {
    pub fn runtime_value(&self) -> DynValue {
        DynValue::obj(&[(self.variant.as_str(), self.payload.clone())])
    }
}

/// Providers implement both enrollment and authentication without leaking
/// provider-specific types into enrollment or session APIs.
pub trait DeviceAuthenticator {
    /// Variant name of the device's attestation and credential in the
    /// runtime's enums. A method rather than a constant so that one type can
    /// stand for devices of several kinds.
    fn variant(&self) -> &'static str;

    async fn attest(&self, request: &ChallengeRequest) -> Result<DeviceAttestation>;

    async fn assert(&self, request: &ChallengeRequest) -> Result<DynValue>;
}

/// Non-secret enrollment state. It remains retryable until the challenge
/// window closes and becomes a profile only after finalized success.
#[derive(Clone, Debug)]
pub struct EnrollmentDraft {
    pub user_id: HashedUserId,
    pub predicted_account: Account,
    pub attestation: DeviceAttestation,
    pub call: PreparedCall,
    pub checkpoint_number: u64,
    pub checkpoint_hash: [u8; 32],
    pub valid_through: u64,
}

/// Build the registration draft locally, prompting `authenticator` once.
pub async fn prepare_enrollment<A: DeviceAuthenticator>(
    metadata: &Metadata,
    config: &PassRuntimeConfig,
    user_id: HashedUserId,
    context: u32,
    block_hash: [u8; 32],
    authenticator: &A,
) -> Result<EnrollmentDraft> {
    // Check before prompting the device for an attestation the runtime
    // would reject.
    if !config.supports_attestation(authenticator.variant()) {
        return Err(Error::Mapping(format!(
            "runtime does not advertise {} attestations",
            authenticator.variant()
        )));
    }
    let predicted_account = config.derive_account(user_id);
    let request = ChallengeRequest::enrollment(
        user_id,
        predicted_account,
        config.authority_id,
        context,
        &block_hash,
    );
    let attestation = authenticator.attest(&request).await?;

    let call = sube::extrinsic::prepare_call(
        metadata,
        &format!("{}/{}", config.pallet, config.register.name),
        &body(vec![
            ("user", DynValue::from(user_id.0)),
            ("attestation", attestation.runtime_value()),
        ]),
    )?;

    Ok(EnrollmentDraft {
        user_id,
        predicted_account,
        attestation,
        call,
        checkpoint_number: u64::from(context),
        checkpoint_hash: block_hash,
        // Kreivo's challenger accepts the current and previous two contexts.
        valid_through: u64::from(context).saturating_add(2),
    })
}

/// Prepare `Pass.add_device_with_filter`. An `Admin` filter needs
/// `admin_confirmed`: this is the one place that enforces it.
///
/// Runtimes that only have the filterless `add_device` grant every device
/// Admin, so a scoped device is refused there.
pub fn prepare_add_device(
    metadata: &Metadata,
    config: &PassRuntimeConfig,
    attestation: &DeviceAttestation,
    filter: &DeviceFilter,
    admin_confirmed: bool,
) -> Result<PreparedCall> {
    if matches!(filter, DeviceFilter::Admin) && !admin_confirmed {
        return Err(Error::OperationFailed(
            "Admin device filter requires separate confirmation".into(),
        ));
    }
    if !config.supports_attestation(&attestation.variant) {
        return Err(Error::Mapping(format!(
            "runtime does not advertise {} attestations",
            attestation.variant
        )));
    }

    let mut fields = vec![("attestation", attestation.runtime_value())];
    if config.add_device.has_field("filter") {
        fields.push(("filter", filter.runtime_value()));
    } else if !matches!(filter, DeviceFilter::Admin) {
        return Err(Error::Mapping(format!(
            "{}::{} takes no device filter; refusing to add an unrestricted device",
            config.pallet, config.add_device.name
        )));
    }
    sube::extrinsic::prepare_call(
        metadata,
        &format!("{}/{}", config.pallet, config.add_device.name),
        &body(fields),
    )
}

pub fn prepare_remove_device(
    metadata: &Metadata,
    config: &PassRuntimeConfig,
    device_id: DeviceId,
) -> Result<PreparedCall> {
    sube::extrinsic::prepare_call(
        metadata,
        &format!("{}/{}", config.pallet, config.remove_device),
        &body(vec![("device_id", DynValue::from(device_id.0))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn signed_message_is_context_challenge_authority() {
        let request = ChallengeRequest {
            user_id: HashedUserId([1; 32]),
            authority_id: AuthorityId([3; 32]),
            context: 0x0403_0201,
            challenge: [2; 32],
        };
        let mut expected = vec![1, 2, 3, 4];
        expected.extend([2; 32]);
        expected.extend([3; 32]);
        assert_eq!(request.signed_message(), expected);
    }

    /// Fails the test if prompted.
    struct Unprompted;

    impl DeviceAuthenticator for Unprompted {
        fn variant(&self) -> &'static str {
            "Nostr"
        }

        async fn attest(&self, _: &ChallengeRequest) -> Result<DeviceAttestation> {
            panic!("the device must not be prompted")
        }

        async fn assert(&self, _: &ChallengeRequest) -> Result<DynValue> {
            panic!("the device must not be prompted")
        }
    }

    #[test]
    fn unsupported_devices_are_refused_before_prompting() {
        let metadata =
            Metadata::from_bytes(include_bytes!("../../sube/tests/fixtures/kreivo.scale")).unwrap();
        let config = PassRuntimeConfig::discover(&metadata).unwrap();
        let error = futures_lite::future::block_on(prepare_enrollment(
            &metadata,
            &config,
            HashedUserId([1; 32]),
            42,
            [2; 32],
            &Unprompted,
        ))
        .unwrap_err();
        assert!(error.to_string().contains("Nostr"), "{error}");
    }

    #[test]
    fn admin_device_needs_separate_confirmation() {
        let metadata =
            Metadata::from_bytes(include_bytes!("../../sube/tests/fixtures/kreivo.scale")).unwrap();
        let config = PassRuntimeConfig::discover(&metadata).unwrap();
        let attestation = DeviceAttestation {
            device_id: DeviceId([1; 32]),
            variant: "WebAuthn".into(),
            payload: DynValue::Null,
        };
        let error = prepare_add_device(
            &metadata,
            &config,
            &attestation,
            &DeviceFilter::Admin,
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("separate confirmation"));
    }

    fn substrate_key_attestation() -> DeviceAttestation {
        DeviceAttestation {
            device_id: DeviceId([1; 32]),
            variant: "SubstrateKey".into(),
            payload: DynValue::obj(&[
                ("public", DynValue::from([1u8; 32])),
                (
                    "message",
                    DynValue::obj(&[
                        ("context", DynValue::from(4u32)),
                        ("challenge", DynValue::from([9u8; 32])),
                        ("authority_id", DynValue::from([3u8; 32])),
                    ]),
                ),
                (
                    "signature",
                    DynValue::obj(&[("Sr25519", DynValue::from([7u8; 64].as_slice()))]),
                ),
            ]),
        }
    }

    #[test]
    fn scoped_devices_are_added_with_their_filter() {
        // Plain `add_device` would grant the device Admin.
        let metadata =
            Metadata::from_bytes(include_bytes!("../../sube/tests/fixtures/kreivo.scale")).unwrap();
        let config = PassRuntimeConfig::discover(&metadata).unwrap();
        let filter = DeviceFilter::Pallets(vec![10]);
        let call = prepare_add_device(
            &metadata,
            &config,
            &substrate_key_attestation(),
            &filter,
            false,
        )
        .unwrap();
        assert!(call.call.eq_ignore_ascii_case("add_device_with_filter"));

        // Pass (6) / add_device_with_filter (5)
        let mut expected = vec![6, 5];
        // Attestation::SubstrateKey (1) { public, message, signature }
        expected.push(1);
        expected.extend([1; 32]);
        expected.extend(4u32.to_le_bytes());
        expected.extend([9; 32]);
        expected.extend([3; 32]);
        // MultiSignature::Sr25519 (1)
        expected.push(1);
        expected.extend([7; 64]);
        // DeviceFilter::Pallets (1) { 10 }
        expected.extend([1, 4, 10]);
        assert_eq!(call.bytes, expected);
    }

    #[test]
    fn scoped_devices_are_refused_without_a_filter_argument() {
        let metadata =
            Metadata::from_bytes(include_bytes!("../../sube/tests/fixtures/kreivo.scale")).unwrap();
        let mut config = PassRuntimeConfig::discover(&metadata).unwrap();
        config.add_device.name = "add_device".into();
        config.add_device.fields.retain(|field| field != "filter");
        let error = prepare_add_device(
            &metadata,
            &config,
            &substrate_key_attestation(),
            &DeviceFilter::Pallets(vec![10]),
            false,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("refusing to add an unrestricted device"),
            "{error}"
        );
    }
}
