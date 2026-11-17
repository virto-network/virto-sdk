use anyhow::Result;

#[cfg(feature = "pass")]
use crate::profiles;

#[cfg(feature = "wallet")]
pub fn import_wallet_profile(
    profile_path: &std::path::Path,
    genesis_hash: [u8; 32],
    name: String,
    mnemonic: &str,
) -> Result<crate::profiles::Profiles> {
    use libwallet::{MutableKeyStore, Pair};

    let mnemonic: libwallet::Mnemonic = mnemonic
        .parse()
        .map_err(|_| anyhow::anyhow!("mnemonic is invalid"))?;
    let seed = libwallet::seed_from_entropy(mnemonic.entropy(), "");
    let root = libwallet::vault::utils::RootAccount::from_bytes(&*seed)
        .ok_or_else(|| anyhow::anyhow!("could not derive wallet root"))?;
    let account: [u8; 32] = root
        .derive("//default")
        .public()
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("wallet signer is not AccountId32"))?;

    let mut profiles = crate::profiles::Profiles::load(profile_path)?;
    if profiles.find(&name).is_some() {
        anyhow::bail!("profile {name:?} already exists");
    }
    let profile = crate::profiles::WalletProfile {
        name,
        genesis_hash,
        account,
    };
    let mut keys = libwallet::vault::OSKeyring::<()>::new(&profile.secure_entry(), None);
    keys.update(mnemonic.phrase())?;
    profiles.upsert(crate::profiles::Profile::Wallet(profile));
    if let Err(error) = profiles.save(profile_path) {
        let _ = keys.delete();
        return Err(error);
    }
    Ok(profiles)
}

#[cfg(feature = "wallet")]
pub async fn activate_existing_profile(
    chain: &mut sube::Sube,
    profile_path: &std::path::Path,
    genesis_hash: [u8; 32],
    name: &str,
) -> Result<crate::profiles::Profiles> {
    let mut profiles = crate::profiles::Profiles::load(profile_path)?;
    let selected = profiles
        .find(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("profile {name:?} not found"))?;
    if selected.genesis_hash() != genesis_hash {
        anyhow::bail!("profile genesis hash does not match the connected chain");
    }
    match selected {
        crate::profiles::Profile::Wallet(profile) => {
            let _ = crate::signers::wallet_signer(&profile).await?;
        }
        #[cfg(feature = "pass")]
        crate::profiles::Profile::Pass(profile) => {
            use sube::Backend;

            let session = profile.session.as_ref().ok_or_else(|| {
                anyhow::anyhow!("pass profile requires a scoped session before activation")
            })?;
            let checkpoint = chain.backend().block_info(None).await?;
            if session.expires_at <= checkpoint.number {
                anyhow::bail!("pass session expired; register or update it before activation");
            }
            let policy = pass::DeviceFilter::parse(chain.metadata(), &session.policy)?;
            if !on_chain_session_matches(chain, &profile, session, &policy).await {
                anyhow::bail!(
                    "local pass session does not exactly match on-chain storage; register or update it before activation"
                );
            }
            let _ = crate::signers::load_session_signer(&profile.session_entry(), session.account)
                .await?;
        }
    }
    profiles.activate(name, genesis_hash)?;
    profiles.save(profile_path)?;
    Ok(profiles)
}

#[cfg(feature = "pass")]
pub enum SessionPreparation {
    Reused(crate::profiles::Profiles),
    Review(Box<Staged>),
}

/// The local change a staged transaction makes once it finalizes, applied
/// to the named profile as stored at that time.
#[cfg(feature = "pass")]
pub enum PendingEffect {
    /// Record `session` and connect the profile; `new_seed` is the session
    /// key to store when it is not stored yet.
    Session {
        profile: String,
        session: crate::profiles::SessionProfile,
        new_seed: Option<zeroize::Zeroizing<[u8; 32]>>,
    },
    /// The new pass profile.
    Enrollment(Box<crate::profiles::PassProfile>),
    DeviceAddition {
        profile: String,
        device: crate::profiles::DeviceRecord,
    },
    DeviceRemoval {
        profile: String,
        device_id: [u8; 32],
    },
}

/// Without pass support no transaction changes local state.
#[cfg(not(feature = "pass"))]
pub enum PendingEffect {}

#[cfg(feature = "pass")]
impl PendingEffect {
    pub fn finalized_message(&self) -> &'static str {
        match self {
            Self::Session { .. } => "Finalized session persisted and profile connected.",
            Self::Enrollment { .. } => "Finalized enrollment persisted as a pass profile.",
            Self::DeviceAddition { .. } => "Finalized device addition persisted.",
            Self::DeviceRemoval { .. } => "Finalized device removal persisted.",
        }
    }
}

#[cfg(feature = "pass")]
pub struct EnrollmentRequest {
    pub name: String,
    pub user_id: String,
    pub registrar: String,
    pub device: crate::profiles::DeviceProviderProfile,
}

#[cfg(feature = "pass")]
pub struct DeviceAdditionRequest {
    pub profile: String,
    pub device: crate::profiles::DeviceProviderProfile,
    pub filter: String,
    pub admin_confirmed: bool,
}

#[cfg(feature = "pass")]
pub struct DeviceRemovalRequest {
    pub profile: String,
    pub device_id: [u8; 32],
}

#[cfg(feature = "pass")]
pub async fn prepare_enrollment(
    chain: &mut sube::Sube,
    chain_url: &str,
    profile_path: &std::path::Path,
    genesis_hash: [u8; 32],
    request: EnrollmentRequest,
) -> Result<Staged> {
    use sube::Backend;

    let profiles = crate::profiles::Profiles::load(profile_path)?;
    let config = pass::PassRuntimeConfig::discover(chain.metadata())?;
    let checkpoint = chain.backend().block_info(None).await?;
    let context = u32::try_from(checkpoint.number)
        .map_err(|_| anyhow::anyhow!("pass context exceeds u32"))?;
    let user_id =
        pass::HashedUserId::from_exact(&hex::decode(request.user_id.trim_start_matches("0x"))?)?;
    if profiles.find(&request.name).is_some() {
        anyhow::bail!("profile {:?} already exists", request.name);
    }
    let registrar_profile = wallet_profile(&profiles, &request.registrar, genesis_hash)?.clone();
    // Checks the runtime accepts the device before prompting it.
    let device = crate::device::Device::open(&profiles, genesis_hash, &request.device).await?;
    let draft = pass::workflow::prepare_enrollment(
        chain.metadata(),
        &config,
        user_id,
        context,
        checkpoint.hash,
        &device,
    )
    .await?;
    let registrar = crate::signers::wallet_signer(&registrar_profile).await?;
    let prepared = prepare_transaction(
        chain,
        chain_url,
        &draft.call,
        &request.registrar,
        &registrar,
        sube::TransactionOptions::default(),
    )
    .await?;
    Ok(Staged {
        prepared,
        notes: vec![
            format!(
                "Predicted pass account: 0x{}",
                hex::encode(draft.predicted_account.0)
            ),
            format!("Enrollment valid through #{}", draft.valid_through),
        ],
        effect: Some(PendingEffect::Enrollment(Box::new(
            crate::profiles::PassProfile {
                name: request.name,
                genesis_hash,
                pass_account: draft.predicted_account.0,
                user_id: user_id.0,
                device_id: draft.attestation.device_id.0,
                device: device.stored_profile()?,
                additional_devices: Vec::new(),
                session: None,
            },
        ))),
    })
}

#[cfg(feature = "pass")]
pub async fn prepare_device_addition(
    chain: &mut sube::Sube,
    chain_url: &str,
    profile_path: &std::path::Path,
    genesis_hash: [u8; 32],
    request: DeviceAdditionRequest,
) -> Result<Staged> {
    use pass::DeviceAuthenticator;
    use sube::Backend;

    let profiles = crate::profiles::Profiles::load(profile_path)?;
    let profile = pass_profile(&profiles, &request.profile, genesis_hash)?.clone();
    let config = pass::PassRuntimeConfig::discover(chain.metadata())?;
    let checkpoint = chain.backend().block_info(None).await?;
    let context = u32::try_from(checkpoint.number)
        .map_err(|_| anyhow::anyhow!("pass context exceeds u32"))?;
    let filter = pass::DeviceFilter::parse(chain.metadata(), &request.filter)?;
    let device = crate::device::Device::open(&profiles, genesis_hash, &request.device).await?;
    let variant = device.variant();
    if !config.supports_attestation(variant) {
        anyhow::bail!("runtime does not advertise {variant} device registration");
    }
    let attestation_request = pass::workflow::ChallengeRequest::enrollment(
        pass::HashedUserId(profile.user_id),
        pass::Account(profile.pass_account),
        config.authority_id,
        context,
        &checkpoint.hash,
    );
    let attestation = device.attest(&attestation_request).await?;
    let call = pass::workflow::prepare_add_device(
        chain.metadata(),
        &config,
        &attestation,
        &filter,
        request.admin_confirmed,
    )?;
    let prepared = prepare_profile_device_transaction(
        chain,
        chain_url,
        &profiles,
        &profile,
        &config,
        context,
        checkpoint.hash,
        &call,
    )
    .await?;
    Ok(Staged {
        prepared,
        notes: vec![
            format!("New device ID: 0x{}", hex::encode(attestation.device_id.0)),
            // Kreivo's challenger accepts the current and previous two contexts.
            format!(
                "Device addition valid through #{}",
                checkpoint.number.saturating_add(2)
            ),
        ],
        effect: Some(PendingEffect::DeviceAddition {
            profile: profile.name,
            device: crate::profiles::DeviceRecord {
                device_id: attestation.device_id.0,
                device: device.stored_profile()?,
            },
        }),
    })
}

#[cfg(feature = "pass")]
pub async fn prepare_device_removal(
    chain: &mut sube::Sube,
    chain_url: &str,
    profile_path: &std::path::Path,
    genesis_hash: [u8; 32],
    request: DeviceRemovalRequest,
) -> Result<Staged> {
    use sube::Backend;

    let profiles = crate::profiles::Profiles::load(profile_path)?;
    let profile = pass_profile(&profiles, &request.profile, genesis_hash)?.clone();
    let known = profile.device_id == request.device_id
        || profile
            .additional_devices
            .iter()
            .any(|device| device.device_id == request.device_id);
    if !known {
        anyhow::bail!("selected device is not recorded in the local pass profile");
    }
    let config = pass::PassRuntimeConfig::discover(chain.metadata())?;
    let checkpoint = chain.backend().block_info(None).await?;
    let context = u32::try_from(checkpoint.number)
        .map_err(|_| anyhow::anyhow!("pass context exceeds u32"))?;
    let call = pass::workflow::prepare_remove_device(
        chain.metadata(),
        &config,
        pass::DeviceId(request.device_id),
    )?;
    let prepared = prepare_profile_device_transaction(
        chain,
        chain_url,
        &profiles,
        &profile,
        &config,
        context,
        checkpoint.hash,
        &call,
    )
    .await?;
    let notes = if request.device_id == profile.device_id {
        vec![
            "Warning: this removes the primary device; the first additional device, if any, becomes primary. Without one the profile becomes unusable.".into(),
        ]
    } else {
        Vec::new()
    };
    Ok(Staged {
        prepared,
        notes,
        effect: Some(PendingEffect::DeviceRemoval {
            profile: profile.name,
            device_id: request.device_id,
        }),
    })
}

#[cfg(feature = "pass")]
pub async fn prepare_pass_session(
    chain: &mut sube::Sube,
    chain_url: &str,
    profile_path: &std::path::Path,
    genesis_hash: [u8; 32],
    name: &str,
    policy_text: &str,
    requested_duration: Option<u32>,
) -> Result<SessionPreparation> {
    use sube::{Backend, Signer};

    let mut profiles = crate::profiles::Profiles::load(profile_path)?;
    let profile = match profiles.find(name).cloned() {
        Some(crate::profiles::Profile::Pass(profile)) => profile,
        _ => anyhow::bail!("pass profile {name:?} not found"),
    };
    if profile.genesis_hash != genesis_hash {
        anyhow::bail!("profile genesis hash does not match the connected chain");
    }
    let policy = pass::DeviceFilter::parse(chain.metadata(), policy_text)?;
    let checkpoint = chain.backend().block_info(None).await?;

    if let Some(session) = &profile.session {
        let local_policy = pass::DeviceFilter::parse(chain.metadata(), &session.policy)?;
        // The runtime stores the filter as sets, so compare it in that form.
        if local_policy.runtime_value() == policy.runtime_value()
            && session.expires_at > checkpoint.number
            && on_chain_session_matches(chain, &profile, session, &policy).await
            && crate::signers::load_session_signer(&profile.session_entry(), session.account)
                .await
                .is_ok()
        {
            profiles.activate(name, genesis_hash)?;
            profiles.save(profile_path)?;
            return Ok(SessionPreparation::Reused(profiles));
        }
    }

    let config = pass::PassRuntimeConfig::discover(chain.metadata())?;
    let duration = pass::session::session_duration(&config, requested_duration)?;
    let existing_account = profile.session.as_ref().map(|session| session.account);
    let mut notes = Vec::new();
    let (session_signer, new_seed) = if let Some(expected) = existing_account {
        match crate::signers::load_session_signer(&profile.session_entry(), expected).await {
            Ok(signer) => (signer, None),
            Err(error) => {
                notes.push(format!(
                    "Stored session key is unavailable or invalid ({error}); registering a replacement"
                ));
                let (signer, seed) = crate::signers::generate_session_signer().await?;
                (signer, Some(zeroize::Zeroizing::new(seed)))
            }
        }
    } else {
        let (signer, seed) = crate::signers::generate_session_signer().await?;
        (signer, Some(zeroize::Zeroizing::new(seed)))
    };
    let session_account = session_signer.account();
    let call = pass::session::prepare_add_session_key(
        chain.metadata(),
        &config,
        session_account,
        &policy,
        Some(duration),
    )?;
    let context = u32::try_from(checkpoint.number)
        .map_err(|_| anyhow::anyhow!("pass context exceeds u32"))?;
    let prepared = prepare_profile_device_transaction(
        chain,
        chain_url,
        &profiles,
        &profile,
        &config,
        context,
        checkpoint.hash,
        &call,
    )
    .await?;
    let effect = PendingEffect::Session {
        profile: profile.name,
        session: crate::profiles::SessionProfile {
            account: session_account,
            policy: policy_text.into(),
            expires_at: checkpoint.number.saturating_add(u64::from(duration)),
        },
        new_seed,
    };
    Ok(SessionPreparation::Review(Box::new(Staged {
        prepared,
        notes,
        effect: Some(effect),
    })))
}

#[cfg(feature = "pass")]
pub fn apply_finalized_effect(
    profile_path: &std::path::Path,
    receipt: &sube::TransactionReceipt,
    effect: PendingEffect,
) -> Result<crate::profiles::Profiles> {
    use libwallet::MutableKeyStore;

    if receipt.finalized_block_hash.is_none()
        || !matches!(receipt.dispatch_outcome, sube::DispatchOutcome::Success)
    {
        anyhow::bail!("operation did not finalize successfully; local profile remains unchanged");
    }
    let mut profiles = crate::profiles::Profiles::load(profile_path)?;
    match effect {
        PendingEffect::Session {
            profile: name,
            session,
            new_seed,
        } => {
            let profile = profiles.pass_mut(&name)?;
            profile.session = Some(session);
            let (genesis_hash, entry) = (profile.genesis_hash, profile.session_entry());
            profiles.activate(&name, genesis_hash)?;
            let mut keys = libwallet::vault::OSKeyring::<()>::new(&entry, None);
            if let Some(seed) = &new_seed {
                keys.upsert(seed.as_slice())?;
            }
            if let Err(error) = profiles.save(profile_path) {
                // Do not keep a key the profile does not refer to.
                if new_seed.is_some() {
                    let _ = keys.delete();
                }
                return Err(error);
            }
        }
        PendingEffect::Enrollment(profile) => {
            profiles.upsert(crate::profiles::Profile::Pass(*profile));
            profiles.save(profile_path)?;
        }
        PendingEffect::DeviceAddition { profile, device } => {
            let profile = profiles.pass_mut(&profile)?;
            profile
                .additional_devices
                .retain(|known| known.device_id != device.device_id);
            profile.additional_devices.push(device);
            profiles.save(profile_path)?;
        }
        PendingEffect::DeviceRemoval { profile, device_id } => {
            profiles.pass_mut(&profile)?.remove_device(device_id);
            profiles.save(profile_path)?;
        }
    }
    Ok(profiles)
}

#[cfg(feature = "pass")]
/// Delete the local session secret of `name`. `genesis_hash`, when known,
/// must match the profile's chain; forgetting needs no connection.
pub fn forget_pass_session(
    profile_path: &std::path::Path,
    genesis_hash: Option<[u8; 32]>,
    name: &str,
) -> Result<crate::profiles::Profiles> {
    use libwallet::MutableKeyStore;

    let mut profiles = crate::profiles::Profiles::load(profile_path)?;
    let profile = profiles.pass_mut(name)?;
    if genesis_hash.is_some_and(|genesis_hash| genesis_hash != profile.genesis_hash) {
        anyhow::bail!("profile genesis hash does not match the connected chain");
    }
    let mut keys = libwallet::vault::OSKeyring::<()>::new(&profile.session_entry(), None);
    keys.delete()?;
    profile.session = None;
    profiles.save(profile_path)?;
    Ok(profiles)
}

/// Sign `call` as `profile` through its primary device.
#[cfg(feature = "pass")]
#[allow(clippy::too_many_arguments)]
pub async fn prepare_profile_device_transaction(
    chain: &mut sube::Sube,
    chain_url: &str,
    profiles: &crate::profiles::Profiles,
    profile: &crate::profiles::PassProfile,
    config: &pass::PassRuntimeConfig,
    context: u32,
    block_hash: [u8; 32],
    call: &sube::PreparedCall,
) -> Result<PreparedTransaction> {
    use pass::DeviceAuthenticator;

    let device =
        crate::device::Device::open(profiles, profile.genesis_hash, &profile.device).await?;
    let variant = device.variant();
    if !config.supports_credential(variant) {
        anyhow::bail!("runtime does not advertise {variant} authentication");
    }
    let authorizer = pass::PassAuthenticator::new(
        &device,
        pass::Account(profile.pass_account),
        pass::DeviceId(profile.device_id),
        pass::HashedUserId(profile.user_id),
        config.authority_id,
        context,
        block_hash,
    );
    prepare_transaction(
        chain,
        chain_url,
        call,
        &profile.name,
        &authorizer,
        sube::TransactionOptions::default(),
    )
    .await
}

/// A signed transaction to review, with the notes shown above it and the
/// local change to apply once it finalizes.
pub struct Staged {
    pub prepared: PreparedTransaction,
    pub notes: Vec<String>,
    pub effect: Option<PendingEffect>,
}

impl Staged {
    pub fn review_text(&self) -> String {
        let mut text = String::new();
        for note in &self.notes {
            text.push_str(note);
            text.push('\n');
        }
        if !self.notes.is_empty() {
            text.push('\n');
        }
        text.push_str(&self.prepared.review_text());
        text
    }
}

/// Sign `call` with the active profile: a wallet directly, or a pass
/// profile through its local session. `None` when no profile is active.
#[cfg(feature = "wallet")]
pub async fn prepare_with_active_profile(
    chain: &mut sube::Sube,
    chain_url: &str,
    profile_path: &std::path::Path,
    call: &sube::PreparedCall,
) -> Result<Option<PreparedTransaction>> {
    let profiles = crate::profiles::Profiles::load(profile_path)?;
    let Some(profile) = profiles.active() else {
        return Ok(None);
    };
    if profile.genesis_hash() != chain_genesis(chain).await? {
        anyhow::bail!("active profile genesis hash does not match the connected chain");
    }
    let options = sube::TransactionOptions::default();
    let prepared = match profile {
        crate::profiles::Profile::Wallet(profile) => {
            let signer = crate::signers::wallet_signer(profile).await?;
            prepare_transaction(chain, chain_url, call, &profile.name, &signer, options).await?
        }
        #[cfg(feature = "pass")]
        crate::profiles::Profile::Pass(profile) => {
            let session = profile.session.as_ref().ok_or_else(|| {
                anyhow::anyhow!("active pass profile has no finalized local session")
            })?;
            let policy = pass::DeviceFilter::parse(chain.metadata(), &session.policy)?;
            let signer =
                crate::signers::load_session_signer(&profile.session_entry(), session.account)
                    .await?;
            let authorizer =
                pass::SessionAuthorizer::new(pass::Account(profile.pass_account), signer, policy)?;
            prepare_transaction(chain, chain_url, call, &profile.name, &authorizer, options).await?
        }
    };
    Ok(Some(prepared))
}

/// One signed transaction together with the non-mutating diagnostics shown
/// before submission.
pub struct PreparedTransaction {
    pub transaction: sube::EncodedExtrinsic,
    pub report: sube::TransactionReport,
    pub authorizer: String,
    pub chain_url: String,
}

impl PreparedTransaction {
    pub fn artifact(&self) -> serde_json::Value {
        transaction_artifact(
            &self.chain_url,
            &self.authorizer,
            &self.transaction,
            &self.report,
        )
    }

    pub fn review_text(&self) -> String {
        review_text(
            &self.chain_url,
            &self.authorizer,
            &self.transaction,
            &self.report,
        )
    }

    pub fn known_invalid(&self) -> bool {
        matches!(
            self.report.validity,
            Some(sube::TransactionValidity::Invalid(_))
        )
    }
}

pub async fn prepare_transaction(
    chain: &mut sube::Sube,
    chain_url: &str,
    call: &sube::PreparedCall,
    authorizer_name: &str,
    authorizer: &(impl sube::ExtrinsicAssembler + ?Sized),
    options: sube::TransactionOptions,
) -> Result<PreparedTransaction> {
    let transaction = chain.build_transaction(call, authorizer, options).await?;
    // A known-invalid transaction is still reviewed; `submit_transaction`
    // refuses to send it.
    let report = chain.inspect_transaction(&transaction).await?;
    Ok(PreparedTransaction {
        transaction,
        report,
        authorizer: authorizer_name.into(),
        chain_url: chain_url.into(),
    })
}

pub async fn submit_transaction(
    chain: &mut sube::Sube,
    prepared: &PreparedTransaction,
    wait_for: sube::WaitFor,
) -> Result<sube::TransactionReceipt> {
    if prepared.known_invalid() {
        anyhow::bail!("transaction validation reports known-invalid; submission blocked");
    }
    chain
        .submit_transaction(&prepared.transaction, wait_for)
        .await
        .map_err(Into::into)
}

pub fn transaction_artifact(
    chain_url: &str,
    authorizer: &str,
    extrinsic: &sube::EncodedExtrinsic,
    report: &sube::TransactionReport,
) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "chain": chain_url,
        "genesisHash": format!("0x{}", hex::encode(extrinsic.genesis_hash)),
        "specVersion": extrinsic.spec_version,
        "transactionVersion": extrinsic.transaction_version,
        "checkpoint": {
            "number": extrinsic.checkpoint_number,
            "hash": format!("0x{}", hex::encode(extrinsic.checkpoint_hash)),
            "expiresAt": extrinsic.expires_at,
        },
        "authorizer": authorizer,
        "signingAccount": format!("0x{}", hex::encode(&extrinsic.authorization.signing_account)),
        "nonceAccount": format!("0x{}", hex::encode(&extrinsic.authorization.nonce_account)),
        "signatureScheme": extrinsic.authorization.scheme,
        "nonce": extrinsic.nonce,
        "call": {
            "pallet": extrinsic.call.pallet,
            "name": extrinsic.call.call,
            "hex": extrinsic.call.hex(),
        },
        "extensions": extrinsic.extensions.iter().map(|extension| serde_json::json!({
            "identifier": extension.identifier,
            "extraHex": extension.extra_hex(),
            "additionalSignedHex": extension.additional_signed_hex(),
        })).collect::<Vec<_>>(),
        "extrinsicHex": extrinsic.hex(),
        "fee": report.partial_fee.map(|fee| fee.to_string()),
        "weight": report.weight.as_ref().map(|weight| serde_json::json!({
            "refTime": weight.ref_time,
            "proofSize": weight.proof_size,
        })),
        "validity": report.validity.as_ref().map(|validity| format!("{validity:?}")),
        "warnings": report.warnings,
        "submitted": false,
    })
}

pub fn review_text(
    chain_url: &str,
    authorizer: &str,
    extrinsic: &sube::EncodedExtrinsic,
    report: &sube::TransactionReport,
) -> String {
    use std::fmt::Write;

    let mut output = String::new();
    let _ = writeln!(output, "Chain: {chain_url}");
    let _ = writeln!(output, "Genesis: 0x{}", hex::encode(extrinsic.genesis_hash));
    let _ = writeln!(output, "Authorizer: {authorizer}");
    let _ = writeln!(
        output,
        "Signing account: 0x{}",
        hex::encode(&extrinsic.authorization.signing_account)
    );
    let _ = writeln!(
        output,
        "Nonce account: 0x{} (nonce {})",
        hex::encode(&extrinsic.authorization.nonce_account),
        extrinsic.nonce
    );
    let _ = writeln!(
        output,
        "Call: {}::{}",
        extrinsic.call.pallet, extrinsic.call.call
    );
    let _ = writeln!(
        output,
        "Checkpoint: #{} 0x{}",
        extrinsic.checkpoint_number,
        hex::encode(extrinsic.checkpoint_hash)
    );
    let _ = writeln!(output, "Expires: {:?}", extrinsic.expires_at);
    let _ = writeln!(output, "Fee: {:?}", report.partial_fee);
    let _ = writeln!(output, "Weight: {:?}", report.weight);
    let _ = writeln!(output, "Validity: {:?}", report.validity);
    let _ = writeln!(output, "Extensions:");
    for extension in &extrinsic.extensions {
        let _ = writeln!(
            output,
            "  {} extra={} additional={}",
            extension.identifier,
            extension.extra_hex(),
            extension.additional_signed_hex()
        );
    }
    let _ = writeln!(output, "Call hex: {}", extrinsic.call.hex());
    let _ = writeln!(output, "Extrinsic hex: {}", extrinsic.hex());
    for warning in &report.warnings {
        let _ = writeln!(output, "Warning: {warning}");
    }
    output
}

pub fn receipt_json(receipt: &sube::TransactionReceipt) -> serde_json::Value {
    serde_json::json!({
        "bestBlockHash": receipt.best_block_hash,
        "finalizedBlockHash": receipt.finalized_block_hash,
        "extrinsicIndex": receipt.extrinsic_index,
        "dispatchOutcome": format!("{:?}", receipt.dispatch_outcome),
        "events": receipt.events.iter().map(|event| serde_json::json!({
            "pallet": event.pallet,
            "variant": event.variant,
            "decoded": event.decoded,
        })).collect::<Vec<_>>(),
    })
}

pub async fn chain_genesis(chain: &mut sube::Sube) -> Result<[u8; 32]> {
    use sube::Backend;
    Ok(chain.backend().block_info(Some(0)).await?.hash)
}

#[cfg(feature = "pass")]
pub async fn on_chain_session_matches(
    chain: &mut sube::Sube,
    profile: &profiles::PassProfile,
    session: &profiles::SessionProfile,
    policy: &pass::DeviceFilter,
) -> bool {
    let Some(expected) = expected_session_value(chain.metadata(), policy) else {
        return false;
    };
    let path = format!("Pass/SessionKeys/0x{}", hex::encode(session.account));
    let Ok(sube::Response::Value(entry, _)) = chain.query(&path).await else {
        return false;
    };
    expected.matches(&entry.data, &profile.pass_account)
}

/// The expected `Pass::SessionKeys` value, which pallet-pass stores as
/// `(pass_account, expires_at, DeviceFilter)`.
#[cfg(feature = "pass")]
#[derive(Debug, PartialEq, Eq)]
pub struct ExpectedSessionValue {
    expiry_len: usize,
    filter: Vec<u8>,
}

#[cfg(feature = "pass")]
impl ExpectedSessionValue {
    /// Compares the owner and the SCALE-encoded filter exactly; the expiry
    /// is set by the runtime and only its width is checked.
    fn matches(&self, data: &[u8], pass_account: &[u8; 32]) -> bool {
        data.len() == 32 + self.expiry_len + self.filter.len()
            && data[..32] == pass_account[..]
            && data[32 + self.expiry_len..] == self.filter[..]
    }
}

/// Encode `filter` as the runtime's `DeviceFilter` stored in `SessionKeys`,
/// with the block-number width taken from metadata. `None` when the layout
/// is not the expected one.
#[cfg(feature = "pass")]
pub fn expected_session_value(
    metadata: &sube::Metadata,
    filter: &pass::DeviceFilter,
) -> Option<ExpectedSessionValue> {
    use sube::scales::TypeDef;
    let entry = metadata
        .pallet_by_name("Pass")?
        .storage
        .as_ref()?
        .entries
        .iter()
        .find(|entry| entry.name == "SessionKeys")?;
    let sube::metadata::StorageEntryType::Map { value, .. } = &entry.ty else {
        return None;
    };
    let registry = &metadata.registry;
    let Some(TypeDef::Tuple(fields)) = registry.resolve(*value) else {
        return None;
    };
    let expiry_len = match registry.resolve(*fields.get(1)?)? {
        TypeDef::U32 => 4,
        TypeDef::U64 => 8,
        _ => return None,
    };
    let filter =
        sube::scales::to_vec_with_info(&filter.runtime_value(), Some((registry, *fields.get(2)?)))
            .ok()?;
    Some(ExpectedSessionValue { expiry_len, filter })
}

#[cfg(feature = "pass")]
pub fn wallet_profile<'a>(
    store: &'a profiles::Profiles,
    name: &str,
    genesis_hash: [u8; 32],
) -> Result<&'a profiles::WalletProfile> {
    match store.find(name) {
        Some(profiles::Profile::Wallet(profile)) if profile.genesis_hash == genesis_hash => {
            Ok(profile)
        }
        Some(profiles::Profile::Wallet(_)) => {
            anyhow::bail!("wallet profile genesis hash does not match the connected chain")
        }
        _ => anyhow::bail!("wallet profile {name:?} not found"),
    }
}

#[cfg(feature = "pass")]
pub fn pass_profile<'a>(
    store: &'a profiles::Profiles,
    name: &str,
    genesis_hash: [u8; 32],
) -> Result<&'a profiles::PassProfile> {
    match store.find(name) {
        Some(profiles::Profile::Pass(profile)) if profile.genesis_hash == genesis_hash => {
            Ok(profile)
        }
        Some(profiles::Profile::Pass(_)) => {
            anyhow::bail!("pass profile genesis hash does not match the connected chain")
        }
        _ => anyhow::bail!("pass profile {name:?} not found"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "pass")]
    fn finalized_receipt() -> sube::TransactionReceipt {
        sube::TransactionReceipt {
            finalized_block_hash: Some("0x01".into()),
            dispatch_outcome: sube::DispatchOutcome::Success,
            ..Default::default()
        }
    }

    #[cfg(feature = "pass")]
    fn test_profile() -> crate::profiles::PassProfile {
        crate::profiles::PassProfile {
            name: "pass".into(),
            genesis_hash: [1; 32],
            pass_account: [2; 32],
            user_id: [3; 32],
            device_id: [4; 32],
            device: crate::profiles::DeviceProviderProfile::SubstrateKey {
                wallet: "primary".into(),
            },
            additional_devices: Vec::new(),
            session: None,
        }
    }

    #[cfg(feature = "pass")]
    fn test_profile_path(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("sube-{label}-{}-{unique}.json", std::process::id()))
    }

    #[test]
    fn artifact_contains_extensions_and_no_secret_fields() {
        let extrinsic = sube::EncodedExtrinsic {
            bytes: vec![1, 2],
            call: sube::PreparedCall {
                pallet: "Balances".into(),
                call: "transfer".into(),
                bytes: vec![5, 0],
            },
            checkpoint_hash: [3; 32],
            checkpoint_number: 10,
            expires_at: Some(64),
            genesis_hash: [4; 32],
            spec_version: 5,
            transaction_version: 6,
            nonce: 7,
            authorization: sube::AuthorizationSummary {
                signing_account: vec![8; 32],
                nonce_account: vec![9; 32],
                scheme: Some("Sr25519".into()),
            },
            extensions: vec![sube::EncodedExtension {
                identifier: "CheckNonce".into(),
                extra: vec![0x1c],
                additional_signed: vec![],
            }],
        };
        let artifact = transaction_artifact(
            "wss://example.invalid",
            "alice",
            &extrinsic,
            &sube::TransactionReport::default(),
        );
        assert_eq!(artifact["extensions"][0]["identifier"], "CheckNonce");
        assert_eq!(
            artifact["signingAccount"],
            format!("0x{}", hex::encode([8; 32]))
        );
        let encoded = artifact.to_string();
        assert!(!encoded.contains("mnemonic"));
        assert!(!encoded.contains("secret"));
    }

    #[cfg(feature = "pass")]
    #[test]
    fn device_changes_persist_only_after_successful_finalization() {
        let path = test_profile_path("device-finalization");
        let profile = test_profile();
        let mut profiles = crate::profiles::Profiles::default();
        profiles.upsert(crate::profiles::Profile::Pass(profile.clone()));
        profiles.save(&path).unwrap();
        let device = crate::profiles::DeviceRecord {
            device_id: [5; 32],
            device: crate::profiles::DeviceProviderProfile::SubstrateKey {
                wallet: "additional".into(),
            },
        };

        let error = apply_finalized_effect(
            &path,
            &sube::TransactionReceipt::default(),
            PendingEffect::DeviceAddition {
                profile: "pass".into(),
                device: device.clone(),
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("remains unchanged"));
        let unchanged = crate::profiles::Profiles::load(&path).unwrap();
        let crate::profiles::Profile::Pass(unchanged) = unchanged.find("pass").unwrap() else {
            panic!("expected pass profile")
        };
        assert!(unchanged.additional_devices.is_empty());

        let changed = apply_finalized_effect(
            &path,
            &finalized_receipt(),
            PendingEffect::DeviceAddition {
                profile: "pass".into(),
                device: device.clone(),
            },
        )
        .unwrap();
        let crate::profiles::Profile::Pass(changed) = changed.find("pass").unwrap() else {
            panic!("expected pass profile")
        };
        assert_eq!(changed.additional_devices.len(), 1);
        assert!(
            changed
                .additional_devices
                .iter()
                .any(|device| device.device_id == [5; 32])
        );

        // Applying the same addition twice records the device once.
        let again = apply_finalized_effect(
            &path,
            &finalized_receipt(),
            PendingEffect::DeviceAddition {
                profile: "pass".into(),
                device,
            },
        )
        .unwrap();
        let crate::profiles::Profile::Pass(again) = again.find("pass").unwrap() else {
            panic!("expected pass profile")
        };
        assert_eq!(again.additional_devices.len(), 1);
        let _ = std::fs::remove_file(path);
    }

    /// An effect names its profile, so changes stored while the transaction
    /// was pending survive it.
    #[cfg(feature = "pass")]
    #[test]
    fn effects_apply_to_the_profile_as_stored_when_they_finalize() {
        let path = test_profile_path("effect-current");
        let record = |id, wallet: &str| crate::profiles::DeviceRecord {
            device_id: [id; 32],
            device: crate::profiles::DeviceProviderProfile::SubstrateKey {
                wallet: wallet.into(),
            },
        };
        let mut profile = test_profile();
        profile
            .additional_devices
            .push(record(6, "added-meanwhile"));
        let mut profiles = crate::profiles::Profiles::default();
        profiles.upsert(crate::profiles::Profile::Pass(profile));
        profiles.save(&path).unwrap();

        let changed = apply_finalized_effect(
            &path,
            &finalized_receipt(),
            PendingEffect::DeviceAddition {
                profile: "pass".into(),
                device: record(5, "additional"),
            },
        )
        .unwrap();
        let crate::profiles::Profile::Pass(changed) = changed.find("pass").unwrap() else {
            panic!("expected pass profile")
        };
        assert_eq!(
            changed.additional_devices,
            [record(6, "added-meanwhile"), record(5, "additional")]
        );
        let _ = std::fs::remove_file(path);
    }

    #[cfg(feature = "pass")]
    #[test]
    fn removing_primary_promotes_one_known_additional_device() {
        let path = test_profile_path("device-promotion");
        let mut profile = test_profile();
        profile
            .additional_devices
            .push(crate::profiles::DeviceRecord {
                device_id: [5; 32],
                device: crate::profiles::DeviceProviderProfile::SubstrateKey {
                    wallet: "replacement".into(),
                },
            });
        let mut profiles = crate::profiles::Profiles::default();
        profiles.upsert(crate::profiles::Profile::Pass(profile.clone()));
        profiles.save(&path).unwrap();
        let changed = apply_finalized_effect(
            &path,
            &finalized_receipt(),
            PendingEffect::DeviceRemoval {
                profile: "pass".into(),
                device_id: [4; 32],
            },
        )
        .unwrap();
        let crate::profiles::Profile::Pass(changed) = changed.find("pass").unwrap() else {
            panic!("expected pass profile")
        };
        assert_eq!(changed.device_id, [5; 32]);
        assert_eq!(
            changed.device,
            crate::profiles::DeviceProviderProfile::SubstrateKey {
                wallet: "replacement".into()
            }
        );
        assert!(changed.additional_devices.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[cfg(feature = "pass")]
    #[test]
    fn session_value_comparison_is_exact() {
        let expected = ExpectedSessionValue {
            expiry_len: 4,
            // Calls variant (index 2) with one (5, 12) pair.
            filter: vec![2, 1 << 2, 5, 12],
        };
        let owner = [9u8; 32];
        let mut stored = owner.to_vec();
        stored.extend(99u32.to_le_bytes());
        stored.extend([2, 1 << 2, 5, 12]);
        assert!(expected.matches(&stored, &owner));
        // Different owner, different filter, or trailing bytes never match.
        assert!(!expected.matches(&stored, &[8u8; 32]));
        let mut other_filter = stored.clone();
        *other_filter.last_mut().unwrap() = 1;
        assert!(!expected.matches(&other_filter, &owner));
        let mut longer = stored.clone();
        longer.push(0);
        assert!(!expected.matches(&longer, &owner));
    }

    /// Read-only live check that the expected `SessionKeys` filter encodes
    /// the same bytes the runtime receives in `add_session_key`.
    #[cfg(feature = "pass")]
    #[test]
    #[ignore = "requires network access to a chain with pallet-pass (SUBE_E2E_URL, default kreivo)"]
    fn live_session_filter_encoding_matches_add_session_key() {
        smol::block_on(async {
            let url = std::env::var("SUBE_E2E_URL").unwrap_or_else(|_| "wss://kreivo.io".into());
            let chain = sube::Sube::connect(&url).await.expect("connects");
            let metadata = chain.metadata();
            let config = pass::PassRuntimeConfig::discover(metadata).expect("pass config");
            for policy in [
                pass::DeviceFilter::Calls(vec![
                    pass::CallIndex {
                        pallet: 10,
                        call: 3,
                    },
                    pass::CallIndex { pallet: 5, call: 0 },
                ]),
                pass::DeviceFilter::Pallets(vec![31, 10]),
            ] {
                let expected = expected_session_value(metadata, &policy).expect("layout");
                let call = pass::session::prepare_add_session_key(
                    metadata, &config, [1; 32], &policy, None,
                )
                .expect("call");
                assert!(
                    call.bytes.ends_with(&expected.filter),
                    "{policy:?}: call {} does not end with {}",
                    call.hex(),
                    hex::encode(&expected.filter)
                );
            }
        })
    }
}
