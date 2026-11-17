//! Provider-neutral enrollment and device-management workflows.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};

use sube::{DynValue, Error, Metadata, PreparedCall, Result};

use crate::config::{PassRuntimeConfig, body};
use crate::{
    Account, AuthorityId, Challenge, DeviceFilter, DeviceId, HashedUserId, enrollment_challenge,
};

/// Enrollment data handed to a device provider.
#[derive(Clone, Debug)]
pub struct AttestationRequest {
    pub user_id: HashedUserId,
    pub pass_account: Account,
    pub authority_id: AuthorityId,
    pub context: u32,
    pub block_hash: [u8; 32],
    pub challenge: Challenge,
}

/// Authentication data handed to a device provider.
#[derive(Clone, Debug)]
pub struct AssertionRequest {
    pub user_id: HashedUserId,
    pub authority_id: AuthorityId,
    pub context: u32,
    pub block_hash: [u8; 32],
    pub binding: [u8; 32],
    pub challenge: Challenge,
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
    async fn attest(&self, request: &AttestationRequest) -> Result<DeviceAttestation>;

    async fn assert(&self, request: &AssertionRequest) -> Result<DynValue>;
}

/// Validate the three-block challenge window before prompting a device.
pub fn ensure_fresh_context(context: u32, finalized: u32) -> Result<()> {
    if finalized < context || finalized.saturating_sub(context) > 2 {
        return Err(Error::OperationFailed(
            "pass authentication challenge is stale".into(),
        ));
    }
    Ok(())
}

/// Non-secret enrollment state. It remains retryable until the challenge
/// window closes and becomes a profile only after finalized success.
#[derive(Clone, Debug)]
pub struct EnrollmentDraft {
    pub registrar_profile: String,
    pub user_id: HashedUserId,
    pub predicted_account: Account,
    pub attestation: DeviceAttestation,
    pub call: PreparedCall,
    pub checkpoint_number: u64,
    pub checkpoint_hash: [u8; 32],
    pub valid_through: u64,
}

impl EnrollmentDraft {
    pub fn is_valid_at(&self, finalized_number: u64) -> bool {
        finalized_number <= self.valid_through
    }
}

/// Build the registration draft locally. The selected registrar is explicit;
/// an empty profile name is rejected rather than silently choosing a wallet.
pub async fn prepare_enrollment<A: DeviceAuthenticator>(
    metadata: &Metadata,
    config: &PassRuntimeConfig,
    registrar_profile: impl Into<String>,
    user_id: HashedUserId,
    context: u32,
    block_hash: [u8; 32],
    authenticator: &A,
) -> Result<EnrollmentDraft> {
    let registrar_profile = registrar_profile.into();
    if registrar_profile.trim().is_empty() {
        return Err(Error::BadInput);
    }
    let predicted_account = config.derive_account(user_id);
    let request = AttestationRequest {
        user_id,
        pass_account: predicted_account,
        authority_id: config.authority_id,
        context,
        block_hash,
        challenge: enrollment_challenge(&block_hash, predicted_account),
    };
    let attestation = authenticator.attest(&request).await?;
    if !config.supports_attestation(&attestation.variant) {
        return Err(Error::Mapping(format!(
            "runtime does not advertise {} attestations",
            attestation.variant
        )));
    }

    let call = sube::extrinsic::prepare_call(
        metadata,
        &format!("{}/{}", config.pallet, config.register.name),
        &body(vec![
            ("user", DynValue::from(user_id.0)),
            ("attestation", attestation.runtime_value()),
        ]),
    )?;

    Ok(EnrollmentDraft {
        registrar_profile,
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

/// Prepare `Pass.add_device`. Requiring `directly_authenticated` makes it
/// impossible for callers to accidentally route this through a session.
pub fn prepare_add_device(
    metadata: &Metadata,
    config: &PassRuntimeConfig,
    attestation: &DeviceAttestation,
    filter: &DeviceFilter,
    directly_authenticated: bool,
    admin_confirmed: bool,
) -> Result<PreparedCall> {
    if !directly_authenticated {
        return Err(Error::OperationFailed(
            "adding a device requires direct device authentication".into(),
        ));
    }
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
    }
    sube::extrinsic::prepare_call(
        metadata,
        &format!("{}/{}", config.pallet, config.add_device.name),
        &body(fields),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceRemovalWarning {
    LastUsableDevice,
    LastAdminDevice,
}

pub fn device_removal_warnings(
    usable_devices: usize,
    admin_devices: usize,
    removing_usable: bool,
    removing_admin: bool,
) -> Vec<DeviceRemovalWarning> {
    let mut warnings = Vec::new();
    if removing_usable && usable_devices <= 1 {
        warnings.push(DeviceRemovalWarning::LastUsableDevice);
    }
    if removing_admin && admin_devices <= 1 {
        warnings.push(DeviceRemovalWarning::LastAdminDevice);
    }
    warnings
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

    struct Canceled;

    impl DeviceAuthenticator for Canceled {
        async fn attest(&self, _: &AttestationRequest) -> Result<DeviceAttestation> {
            Err(Error::Signing("user canceled".into()))
        }

        async fn assert(&self, _: &AssertionRequest) -> Result<DynValue> {
            Err(Error::Signing("user canceled".into()))
        }
    }

    #[test]
    fn enrollment_requires_a_registrar_profile() {
        let metadata =
            Metadata::from_bytes(include_bytes!("../../sube/tests/fixtures/kreivo.scale")).unwrap();
        let config = PassRuntimeConfig::discover(&metadata).unwrap();
        let result = futures_lite::future::block_on(prepare_enrollment(
            &metadata,
            &config,
            "",
            HashedUserId([1; 32]),
            42,
            [2; 32],
            &Canceled,
        ));
        assert!(matches!(result, Err(Error::BadInput)));
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
            true,
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("separate confirmation"));
    }

    #[test]
    fn stale_challenges_are_rejected_before_device_prompt() {
        assert!(ensure_fresh_context(40, 42).is_ok());
        assert!(ensure_fresh_context(40, 43).is_err());
        assert!(ensure_fresh_context(41, 40).is_err());
    }
}
