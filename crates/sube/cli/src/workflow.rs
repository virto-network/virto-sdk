use anyhow::Result;

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
    let secure_entry = format!("wallet-profile-v1-{name}");
    let mut keys = libwallet::vault::OSKeyring::<()>::new(&secure_entry, None);
    keys.update(mnemonic.phrase())?;
    profiles.upsert(crate::profiles::Profile::Wallet(
        crate::profiles::WalletProfile {
            name,
            genesis_hash,
            account,
            secure_entry,
            scheme: "sr25519".into(),
        },
    ));
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
            let _ = crate::wallet_signer(&profile).await?;
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
            let policy = crate::parse_session_policy(chain.metadata(), &session.policy)?;
            if !crate::on_chain_session_matches(chain, &profile, session, &policy).await {
                anyhow::bail!(
                    "local pass session does not exactly match on-chain storage; register or update it before activation"
                );
            }
            let _ =
                crate::load_session_signer(&session.secure_entry, Some(session.account)).await?;
        }
    }
    profiles.activate(name, genesis_hash)?;
    profiles.save(profile_path)?;
    Ok(profiles)
}

#[cfg(feature = "pass")]
pub enum SessionPreparation {
    Reused(crate::profiles::Profiles),
    Review {
        prepared: Box<PreparedTransaction>,
        effect: Box<PendingEffect>,
    },
}

#[cfg(feature = "pass")]
pub enum PendingEffect {
    Session {
        profile: crate::profiles::PassProfile,
        session_account: [u8; 32],
        secure_entry: String,
        policy: String,
        expires_at: u64,
        pending_seed: Option<zeroize::Zeroizing<[u8; 32]>>,
    },
    Enrollment {
        pending: crate::profiles::PendingEnrollment,
    },
}

#[cfg(feature = "pass")]
pub struct EnrollmentRequest {
    pub name: String,
    pub user_id: String,
    pub registrar: String,
    pub device: crate::profiles::DeviceProviderProfile,
}

#[cfg(feature = "pass")]
pub async fn prepare_enrollment(
    chain: &mut sube::Sube,
    chain_url: &str,
    profile_path: &std::path::Path,
    genesis_hash: [u8; 32],
    request: EnrollmentRequest,
) -> Result<(PreparedTransaction, PendingEffect, [u8; 32], u64)> {
    use sube::Backend;

    let mut profiles = crate::profiles::Profiles::load(profile_path)?;
    let config = pass::PassRuntimeConfig::discover(chain.metadata())?;
    let checkpoint = chain.backend().block_info(None).await?;
    let context = u32::try_from(checkpoint.number)
        .map_err(|_| anyhow::anyhow!("pass context exceeds u32"))?;
    let variant = crate::device_variant(&request.device);
    if !config.supports_attestation(variant) {
        anyhow::bail!("runtime does not advertise {variant} enrollment");
    }
    let user_id =
        pass::HashedUserId::from_exact(&hex::decode(request.user_id.trim_start_matches("0x"))?)?;
    if profiles.find(&request.name).is_some() {
        anyhow::bail!("profile {:?} already exists", request.name);
    }
    let registrar_profile =
        crate::wallet_profile(&profiles, &request.registrar, genesis_hash)?.clone();
    let pending = profiles
        .pending_enrollments
        .iter()
        .find(|pending| {
            pending.name == request.name
                && pending.genesis_hash == genesis_hash
                && pending.registrar == request.registrar
                && crate::same_requested_device(&pending.device_profile(), &request.device)
                && pending.user_id == user_id.0
                && pending.valid_through >= checkpoint.number
        })
        .cloned();
    let pending = if let Some(pending) = pending {
        pending
    } else {
        let (draft, enrolled_device) = prepare_enrollment_device(
            chain,
            &config,
            &profiles,
            &request.registrar,
            user_id,
            context,
            checkpoint.hash,
            request.device,
            genesis_hash,
        )
        .await?;
        let legacy_device_wallet = match &enrolled_device {
            crate::profiles::DeviceProviderProfile::SubstrateKey { wallet } => wallet.clone(),
            _ => String::new(),
        };
        let pending = crate::profiles::PendingEnrollment {
            name: request.name,
            genesis_hash,
            registrar: request.registrar,
            device_wallet: legacy_device_wallet,
            device: Some(enrolled_device),
            user_id: user_id.0,
            pass_account: draft.predicted_account.0,
            device_id: draft.attestation.device_id.0,
            pallet: draft.call.pallet,
            call: draft.call.call,
            call_bytes: draft.call.bytes,
            call_hex: draft.call.hex,
            checkpoint_number: draft.checkpoint_number,
            checkpoint_hash: draft.checkpoint_hash,
            valid_through: draft.valid_through,
        };
        profiles.upsert_pending(pending.clone());
        profiles.save(profile_path)?;
        pending
    };
    let call = sube::PreparedCall {
        pallet: pending.pallet.clone(),
        call: pending.call.clone(),
        bytes: pending.call_bytes.clone(),
        hex: pending.call_hex.clone(),
    };
    let registrar = crate::wallet_signer(&registrar_profile).await?;
    let prepared = prepare_transaction(
        chain,
        chain_url,
        &call,
        &pending.registrar,
        &registrar,
        sube::TransactionOptions::default(),
    )
    .await?;
    Ok((
        prepared,
        PendingEffect::Enrollment {
            pending: pending.clone(),
        },
        pending.pass_account,
        pending.valid_through,
    ))
}

#[cfg(feature = "pass")]
#[allow(clippy::too_many_arguments)]
async fn prepare_enrollment_device(
    chain: &mut sube::Sube,
    config: &pass::PassRuntimeConfig,
    profiles: &crate::profiles::Profiles,
    registrar: &str,
    user_id: pass::HashedUserId,
    context: u32,
    block_hash: [u8; 32],
    device: crate::profiles::DeviceProviderProfile,
    genesis_hash: [u8; 32],
) -> Result<(
    pass::EnrollmentDraft,
    crate::profiles::DeviceProviderProfile,
)> {
    match device {
        crate::profiles::DeviceProviderProfile::SubstrateKey { wallet } => {
            let wallet_profile = crate::wallet_profile(profiles, &wallet, genesis_hash)?.clone();
            let signer = crate::wallet_signer(&wallet_profile).await?;
            let public = signer.inner().public();
            let device = pass::wallet::WalletDevice::new(
                signer.inner(),
                public.as_ref(),
                pass::wallet::SignatureType::Sr25519,
            );
            let draft = pass::workflow::prepare_enrollment(
                chain.metadata(),
                config,
                registrar,
                user_id,
                context,
                block_hash,
                &device,
            )
            .await?;
            Ok((
                draft,
                crate::profiles::DeviceProviderProfile::SubstrateKey { wallet },
            ))
        }
        crate::profiles::DeviceProviderProfile::WebAuthn { rp_id, origin, .. } => {
            #[cfg(feature = "desktop-webauthn")]
            {
                let device = pass::webauthn::WebAuthnDevice::for_enrollment(
                    pass::webauthn::DesktopWebAuthnTransport,
                    rp_id,
                    origin,
                );
                let draft = pass::workflow::prepare_enrollment(
                    chain.metadata(),
                    config,
                    registrar,
                    user_id,
                    context,
                    block_hash,
                    &device,
                )
                .await?;
                let profile = device.profile().ok_or_else(|| {
                    anyhow::anyhow!("WebAuthn authenticator returned no credential id")
                })?;
                Ok((
                    draft,
                    crate::profiles::DeviceProviderProfile::WebAuthn {
                        rp_id: profile.rp_id,
                        origin: profile.origin,
                        credential_id: profile.credential_id,
                    },
                ))
            }
            #[cfg(not(feature = "desktop-webauthn"))]
            {
                let _ = (rp_id, origin);
                anyhow::bail!("this build has no desktop WebAuthn support")
            }
        }
        crate::profiles::DeviceProviderProfile::SshAgent {
            fingerprint,
            namespace,
        } => {
            #[cfg(all(feature = "ssh-agent", unix))]
            {
                let transport =
                    pass::ssh_agent::UnixSshAgent::from_env().map_err(anyhow::Error::msg)?;
                let device = pass::ssh_agent::SshAgentDevice::new(
                    transport,
                    fingerprint.clone(),
                    namespace.clone(),
                );
                let draft = pass::workflow::prepare_enrollment(
                    chain.metadata(),
                    config,
                    registrar,
                    user_id,
                    context,
                    block_hash,
                    &device,
                )
                .await?;
                Ok((
                    draft,
                    crate::profiles::DeviceProviderProfile::SshAgent {
                        fingerprint,
                        namespace,
                    },
                ))
            }
            #[cfg(not(all(feature = "ssh-agent", unix)))]
            {
                let _ = (fingerprint, namespace);
                anyhow::bail!("this build has no native SSH-agent support")
            }
        }
    }
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
    let policy = crate::parse_session_policy(chain.metadata(), policy_text)?;
    let checkpoint = chain.backend().block_info(None).await?;

    if let Some(session) = &profile.session {
        let local_policy = crate::parse_session_policy(chain.metadata(), &session.policy)?;
        if local_policy == policy
            && session.expires_at > checkpoint.number
            && crate::on_chain_session_matches(chain, &profile, session, &policy).await
            && crate::load_session_signer(&session.secure_entry, Some(session.account))
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
    let secure_entry =
        libwallet::pass_session_entry_id(&profile.genesis_hash, &profile.pass_account);
    let existing_account = profile.session.as_ref().map(|session| session.account);
    let (session_signer, pending_seed) = if let Some(expected) = existing_account {
        match crate::load_session_signer(&secure_entry, Some(expected)).await {
            Ok(signer) => (signer, None),
            Err(_) => {
                let (signer, seed) = crate::generate_session_signer().await?;
                (signer, Some(zeroize::Zeroizing::new(seed)))
            }
        }
    } else {
        let (signer, seed) = crate::generate_session_signer().await?;
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
        profile,
        session_account,
        secure_entry,
        policy: policy_text.into(),
        expires_at: checkpoint.number.saturating_add(u64::from(duration)),
        pending_seed,
    };
    Ok(SessionPreparation::Review {
        prepared: Box::new(prepared),
        effect: Box::new(effect),
    })
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
    match effect {
        PendingEffect::Session {
            mut profile,
            session_account,
            secure_entry,
            policy,
            expires_at,
            mut pending_seed,
        } => {
            if let Some(seed) = pending_seed.as_deref_mut() {
                let mut keys = libwallet::vault::OSKeyring::<()>::new(&secure_entry, None);
                keys.upsert(seed)?;
            }
            profile.session = Some(crate::profiles::SessionProfile {
                account: session_account,
                secure_entry: secure_entry.clone(),
                policy,
                expires_at,
            });
            let name = profile.name.clone();
            let genesis_hash = profile.genesis_hash;
            let mut profiles = crate::profiles::Profiles::load(profile_path)?;
            profiles.upsert(crate::profiles::Profile::Pass(profile));
            profiles.activate(&name, genesis_hash)?;
            if let Err(error) = profiles.save(profile_path) {
                if pending_seed.is_some() {
                    let mut keys = libwallet::vault::OSKeyring::<()>::new(&secure_entry, None);
                    let _ = keys.delete();
                }
                return Err(error);
            }
            Ok(profiles)
        }
        PendingEffect::Enrollment { pending } => {
            let mut profiles = crate::profiles::Profiles::load(profile_path)?;
            profiles.upsert(crate::profiles::Profile::Pass(
                crate::profiles::PassProfile {
                    name: pending.name.clone(),
                    genesis_hash: pending.genesis_hash,
                    pass_account: pending.pass_account,
                    user_id: pending.user_id,
                    device_id: pending.device_id,
                    device_wallet: pending.device_wallet.clone(),
                    device: Some(pending.device_profile()),
                    additional_devices: Vec::new(),
                    session: None,
                },
            ));
            profiles.remove_pending(&pending.name);
            profiles.save(profile_path)?;
            Ok(profiles)
        }
    }
}

#[cfg(feature = "pass")]
pub fn forget_pass_session(
    profile_path: &std::path::Path,
    genesis_hash: [u8; 32],
    name: &str,
) -> Result<crate::profiles::Profiles> {
    use libwallet::MutableKeyStore;

    let mut profiles = crate::profiles::Profiles::load(profile_path)?;
    let mut profile = match profiles.find(name).cloned() {
        Some(crate::profiles::Profile::Pass(profile)) => profile,
        _ => anyhow::bail!("pass profile {name:?} not found"),
    };
    if profile.genesis_hash != genesis_hash {
        anyhow::bail!("profile genesis hash does not match the connected chain");
    }
    let secure_entry = profile
        .session
        .as_ref()
        .map(|session| session.secure_entry.clone())
        .unwrap_or_else(|| {
            libwallet::pass_session_entry_id(&profile.genesis_hash, &profile.pass_account)
        });
    let mut keys = libwallet::vault::OSKeyring::<()>::new(&secure_entry, None);
    keys.delete()?;
    profile.session = None;
    profiles.upsert(crate::profiles::Profile::Pass(profile));
    profiles.save(profile_path)?;
    Ok(profiles)
}

#[cfg(feature = "pass")]
#[allow(clippy::too_many_arguments)]
async fn prepare_device_authenticated<A: pass::DeviceAuthenticator>(
    chain: &mut sube::Sube,
    chain_url: &str,
    profile: &crate::profiles::PassProfile,
    config: &pass::PassRuntimeConfig,
    context: u32,
    block_hash: [u8; 32],
    call: &sube::PreparedCall,
    device: &A,
) -> Result<PreparedTransaction> {
    let provider = pass::PassAuthorizer::new(
        device,
        pass::HashedUserId(profile.user_id),
        config.authority_id,
        context,
        block_hash,
    )
    .with_challenger(config.challenger);
    let authorizer = pass::PassAuthenticator::new(
        pass::Account(profile.pass_account),
        pass::DeviceId(profile.device_id),
        provider,
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
    let primary_device = profile.primary_device();
    let variant = crate::device_variant(&primary_device);
    if !config.supports_credential(variant) {
        anyhow::bail!("runtime does not advertise {variant} authentication");
    }
    match primary_device {
        crate::profiles::DeviceProviderProfile::SubstrateKey { wallet } => {
            let wallet = crate::wallet_profile(profiles, &wallet, profile.genesis_hash)?.clone();
            let signer = crate::wallet_signer(&wallet).await?;
            let public = signer.inner().public();
            let device = pass::wallet::WalletDevice::new(
                signer.inner(),
                public.as_ref(),
                pass::wallet::SignatureType::Sr25519,
            );
            prepare_device_authenticated(
                chain, chain_url, profile, config, context, block_hash, call, &device,
            )
            .await
        }
        crate::profiles::DeviceProviderProfile::WebAuthn {
            rp_id,
            origin,
            credential_id,
        } => {
            #[cfg(feature = "desktop-webauthn")]
            {
                let device = pass::webauthn::WebAuthnDevice::from_profile(
                    pass::webauthn::DesktopWebAuthnTransport,
                    pass::webauthn::WebAuthnProfile {
                        rp_id,
                        origin,
                        credential_id,
                    },
                );
                prepare_device_authenticated(
                    chain, chain_url, profile, config, context, block_hash, call, &device,
                )
                .await
            }
            #[cfg(not(feature = "desktop-webauthn"))]
            {
                let _ = (rp_id, origin, credential_id);
                anyhow::bail!("this build has no desktop WebAuthn support")
            }
        }
        crate::profiles::DeviceProviderProfile::SshAgent {
            fingerprint,
            namespace,
        } => {
            #[cfg(all(feature = "ssh-agent", unix))]
            {
                let transport =
                    pass::ssh_agent::UnixSshAgent::from_env().map_err(anyhow::Error::msg)?;
                let device =
                    pass::ssh_agent::SshAgentDevice::new(transport, fingerprint, namespace);
                prepare_device_authenticated(
                    chain, chain_url, profile, config, context, block_hash, call, &device,
                )
                .await
            }
            #[cfg(not(all(feature = "ssh-agent", unix)))]
            {
                let _ = (fingerprint, namespace);
                anyhow::bail!("this build has no native SSH-agent support")
            }
        }
    }
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
    let report = chain.inspect_transaction(&transaction).await?;
    if matches!(report.validity, Some(sube::TransactionValidity::Invalid(_))) {
        anyhow::bail!("transaction validation reports known-invalid; submission blocked");
    }
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
            "hex": extrinsic.call.hex,
        },
        "extensions": extrinsic.extensions.iter().map(|extension| serde_json::json!({
            "identifier": extension.identifier,
            "extraHex": extension.extra_hex,
            "additionalSignedHex": extension.additional_signed_hex,
        })).collect::<Vec<_>>(),
        "extrinsicHex": extrinsic.hex,
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
            extension.identifier, extension.extra_hex, extension.additional_signed_hex
        );
    }
    let _ = writeln!(output, "Call hex: {}", extrinsic.call.hex);
    let _ = writeln!(output, "Extrinsic hex: {}", extrinsic.hex);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_contains_extensions_and_no_secret_fields() {
        let extrinsic = sube::EncodedExtrinsic {
            bytes: vec![1, 2],
            hex: "0x0102".into(),
            call: sube::PreparedCall {
                pallet: "Balances".into(),
                call: "transfer".into(),
                bytes: vec![5, 0],
                hex: "0x0500".into(),
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
                extra_hex: "0x1c".into(),
                additional_signed_hex: "0x".into(),
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
}
