//! Provider-neutral enrollment and device-management workflows.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};

use sube::{DispatchOutcome, DynValue, Error, Metadata, PreparedCall, Result, TransactionReceipt};

use crate::config::{PassRuntimeConfig, body};
use crate::{Account, AuthorityId, Challenge, DeviceId, HashedUserId, block_challenge};

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

/// Turns any [`DeviceAuthenticator`] into the credential provider consumed by
/// the generic V5 pass extrinsic assembler.
pub struct PassAuthorizer<'a, A> {
    authenticator: &'a A,
    user_id: HashedUserId,
    authority_id: AuthorityId,
    context: u32,
    block_hash: [u8; 32],
    challenger: crate::config::Challenger,
}

impl<'a, A> PassAuthorizer<'a, A> {
    pub fn new(
        authenticator: &'a A,
        user_id: HashedUserId,
        authority_id: AuthorityId,
        context: u32,
        block_hash: [u8; 32],
    ) -> Self {
        Self {
            authenticator,
            user_id,
            authority_id,
            context,
            block_hash,
            challenger: block_challenge,
        }
    }

    pub fn with_challenger(mut self, challenger: crate::config::Challenger) -> Self {
        self.challenger = challenger;
        self
    }
}

impl<A: DeviceAuthenticator> crate::CredentialProvider for PassAuthorizer<'_, A> {
    async fn credential(&self, binding: &[u8; 32]) -> Result<DynValue> {
        self.authenticator
            .assert(&AssertionRequest {
                user_id: self.user_id,
                authority_id: self.authority_id,
                context: self.context,
                block_hash: self.block_hash,
                binding: *binding,
                challenge: (self.challenger)(&self.block_hash, binding),
            })
            .await
    }
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

    pub fn profile(&self, genesis_hash: [u8; 32]) -> PassProfile {
        PassProfile {
            genesis_hash,
            pass_account: self.predicted_account,
            user_id: self.user_id,
            devices: vec![self.attestation.device_id],
        }
    }

    /// Persist only a finalized successful registration.
    pub fn persist_finalized<S: PassProfileStore>(
        &self,
        genesis_hash: [u8; 32],
        receipt: &TransactionReceipt,
        store: &mut S,
    ) -> core::result::Result<(), EnrollmentCommitError<S::Error>> {
        if receipt.finalized_block_hash.is_none() {
            return Err(EnrollmentCommitError::NotFinalized);
        }
        if !matches!(receipt.dispatch_outcome, DispatchOutcome::Success) {
            return Err(EnrollmentCommitError::DispatchFailed);
        }
        store
            .upsert(self.profile(genesis_hash))
            .map_err(EnrollmentCommitError::Store)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassProfile {
    pub genesis_hash: [u8; 32],
    pub pass_account: Account,
    pub user_id: HashedUserId,
    pub devices: Vec<DeviceId>,
}

pub trait PassProfileStore {
    type Error;

    fn upsert(&mut self, profile: PassProfile) -> core::result::Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnrollmentCommitError<E> {
    NotFinalized,
    DispatchFailed,
    Store(E),
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
        challenge: (config.challenger)(&block_hash, &predicted_account.0),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceFilter {
    Calls(Vec<(u8, u8)>),
    Pallets(Vec<u8>),
    Admin,
}

impl DeviceFilter {
    pub fn runtime_value(&self) -> DynValue {
        match self {
            Self::Calls(calls) => DynValue::obj(&[(
                "Calls",
                DynValue::Seq(
                    calls
                        .iter()
                        .map(|(pallet, call)| {
                            DynValue::Seq(vec![DynValue::from(*pallet), DynValue::from(*call)])
                        })
                        .collect(),
                ),
            )]),
            Self::Pallets(pallets) => DynValue::obj(&[(
                "Pallets",
                DynValue::Seq(pallets.iter().map(|index| DynValue::from(*index)).collect()),
            )]),
            Self::Admin => DynValue::obj(&[("Admin", DynValue::Null)]),
        }
    }
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
        &format!("{}/{}", config.pallet, config.remove_device.name),
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
