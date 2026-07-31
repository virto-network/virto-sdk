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
