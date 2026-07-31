//! Live pallet-pass workflow. Run explicitly with:
//!
//! ```text
//! SUBE_E2E_URL=wss://... \
//! SUBE_E2E_REGISTRAR_MNEMONIC="twelve funded words ..." \
//! SUBE_E2E_DEVICE_MNEMONIC="twelve device words ..." \
//! SUBE_E2E_USER_ID=0x... \
//! cargo test -p pass --test e2e -- --ignored --nocapture
//! ```

use libwallet::vault::{Simple, Vault};
use sube::{Backend, ExtrinsicAssembler};

type WalletSigner = sube::libwallet::LibwalletSigner<libwallet::vault::utils::DerivedSigner>;

#[test]
#[ignore = "requires SUBE_E2E_URL and funded registrar credentials"]
fn funded_substrate_key_enrollment_and_session_workflow() {
    futures_lite::future::block_on(async {
        let url = required_env("SUBE_E2E_URL");
        let registrar_phrase = required_env("SUBE_E2E_REGISTRAR_MNEMONIC");
        let device_phrase = required_env("SUBE_E2E_DEVICE_MNEMONIC");
        let user_id = pass::HashedUserId::from_exact(
            &hex::decode(required_env("SUBE_E2E_USER_ID").trim_start_matches("0x"))
                .expect("SUBE_E2E_USER_ID must be hex"),
        )
        .expect("SUBE_E2E_USER_ID must be exactly 32 bytes");

        let mut chain = sube::Sube::connect(&url).await.expect("connect E2E chain");
        let config =
            pass::PassRuntimeConfig::discover(chain.metadata()).expect("discover Pass runtime");
        let genesis_hash = chain
            .backend()
            .block_info(Some(0))
            .await
            .expect("genesis")
            .hash;

        let (registrar_inner, registrar_account) =
            derived_signer(&registrar_phrase, "//default").await;
        let registrar = WalletSigner::new(
            registrar_inner,
            registrar_account,
            sube::libwallet::SignatureScheme::Sr25519,
        );
        let (device_inner, _) = derived_signer(&device_phrase, "//pass-device").await;
        let device_public = device_inner.public();
        let device = pass::wallet::WalletDevice::new(
            &device_inner,
            device_public.as_ref(),
            pass::wallet::SignatureType::Sr25519,
        );

        let enrollment_checkpoint = chain
            .backend()
            .block_info(None)
            .await
            .expect("finalized enrollment checkpoint");
        let enrollment_context =
            u32::try_from(enrollment_checkpoint.number).expect("block number fits u32");
        let draft = pass::workflow::prepare_enrollment(
            chain.metadata(),
            &config,
            "e2e-registrar",
            user_id,
            enrollment_context,
            enrollment_checkpoint.hash,
            &device,
        )
        .await
        .expect("prepare enrollment");
        eprintln!(
            "predicted pass account: 0x{}",
            hex::encode(draft.predicted_account.0)
        );

        let registration = chain
            .build_transaction(&draft.call, &registrar, sube::TransactionOptions::default())
            .await
            .expect("build registration");
        assert_not_known_invalid(
            chain
                .inspect_transaction(&registration)
                .await
                .expect("inspect registration"),
        );
        let registration_receipt = chain
            .submit_transaction(&registration, sube::WaitFor::Finalized)
            .await
            .expect("submit registration");
        assert_finalized_success(&registration_receipt);

        let (session_inner, session_account) =
            derived_signer(&device_phrase, "//pass-session").await;
        let session_signer = WalletSigner::new(
            session_inner,
            session_account,
            sube::libwallet::SignatureScheme::Sr25519,
        );
        let allowed = chain
            .prepare_call("system/remark", &sube::Text("(remark:0x706173732d653265)"))
            .expect("prepare allowed call");
        let policy = pass::SessionPolicy::this_call(&allowed, chain.metadata())
            .expect("resolve call policy");
        let duration = pass::session::session_duration(&config, None).expect("session duration");
        let add_session = pass::session::prepare_add_session_key(
            chain.metadata(),
            &config,
            session_account,
            &policy,
            Some(duration),
        )
        .expect("prepare session registration");

        let session_checkpoint = chain
            .backend()
            .block_info(None)
            .await
            .expect("finalized session checkpoint");
        let session_context =
            u32::try_from(session_checkpoint.number).expect("block number fits u32");
        let provider = pass::PassAuthorizer::new(
            &device,
            user_id,
            config.authority_id,
            session_context,
            session_checkpoint.hash,
        )
        .with_challenger(config.challenger);
        let direct = pass::PassAuthenticator::new(
            draft.predicted_account,
            draft.attestation.device_id,
            provider,
        );
        let session_registration = chain
            .build_transaction(&add_session, &direct, sube::TransactionOptions::default())
            .await
            .expect("build session registration");
        assert_not_known_invalid(
            chain
                .inspect_transaction(&session_registration)
                .await
                .expect("inspect session registration"),
        );
        let session_receipt = chain
            .submit_transaction(&session_registration, sube::WaitFor::Finalized)
            .await
            .expect("submit session registration");
        assert_finalized_success(&session_receipt);
        assert!(
            session_exists(&mut chain, draft.predicted_account.0, session_account).await,
            "registered session was not found in Pass storage"
        );

        let local = pass::session::LocalSession {
            genesis_hash,
            pass_account: draft.predicted_account,
            session_account,
            policy: policy.clone(),
            expires_at: session_checkpoint.number + u64::from(duration),
        };
        let on_chain = pass::session::OnChainSession {
            pass_account: draft.predicted_account,
            session_account,
            policy: policy.clone(),
            expires_at: local.expires_at,
        };
        assert!(matches!(
            pass::SessionManager::plan(
                Some(local),
                Some(&on_chain),
                draft.predicted_account,
                &policy,
                session_checkpoint.number
            ),
            pass::session::SessionPlan::Reuse(_)
        ));

        let session =
            pass::SessionAuthorizer::new(draft.predicted_account, session_signer, policy.clone());
        assert_eq!(session.nonce_account(), draft.predicted_account.0);
        let allowed_transaction = chain
            .build_transaction(&allowed, &session, sube::TransactionOptions::default())
            .await
            .expect("build allowed session transaction");
        assert_not_known_invalid(
            chain
                .inspect_transaction(&allowed_transaction)
                .await
                .expect("inspect allowed transaction"),
        );
        let allowed_receipt = chain
            .submit_transaction(&allowed_transaction, sube::WaitFor::Finalized)
            .await
            .expect("submit allowed transaction");
        assert_finalized_success(&allowed_receipt);
        assert!(
            !allowed_receipt.events.is_empty(),
            "finalized receipt should contain decoded events"
        );

        let disallowed = chain
            .prepare_call(
                "system/remark_with_event",
                &sube::Text("(remark:0x64656e696564)"),
            )
            .expect("prepare disallowed call");
        let error = chain
            .build_transaction(&disallowed, &session, sube::TransactionOptions::default())
            .await
            .expect_err("local policy must reject a disallowed call");
        assert!(
            error
                .to_string()
                .contains("outside the local session policy")
        );
    });
}

async fn derived_signer(
    phrase: &str,
    path: &'static str,
) -> (libwallet::vault::utils::DerivedSigner, [u8; 32]) {
    let keys = Simple::<(), 16>::from_phrase(phrase)
        .expect("E2E mnemonics must contain 12 words / 128-bit entropy");
    let mut vault = libwallet::Substrate::new(keys);
    let signer = vault.unlock(Some(path), ()).await.expect("derive signer");
    let account = signer
        .public()
        .as_ref()
        .try_into()
        .expect("Substrate signer must use AccountId32");
    (signer, account)
}

fn assert_not_known_invalid(report: sube::TransactionReport) {
    assert!(
        !matches!(report.validity, Some(sube::TransactionValidity::Invalid(_))),
        "transaction is known-invalid: {:?}",
        report.validity
    );
}

fn assert_finalized_success(receipt: &sube::TransactionReceipt) {
    assert!(
        receipt.finalized_block_hash.is_some(),
        "receipt is not finalized"
    );
    assert!(
        matches!(receipt.dispatch_outcome, sube::DispatchOutcome::Success),
        "dispatch failed: {:?}",
        receipt.dispatch_outcome
    );
}

async fn session_exists(
    chain: &mut sube::Sube,
    pass_account: [u8; 32],
    session_account: [u8; 32],
) -> bool {
    let candidates: Vec<(String, usize)> = chain
        .metadata()
        .pallet_by_name("Pass")
        .and_then(|pallet| pallet.storage.as_ref().map(|storage| (pallet, storage)))
        .map(|(pallet, storage)| {
            storage
                .entries
                .iter()
                .filter(|entry| entry.name.to_ascii_lowercase().contains("session"))
                .map(|entry| {
                    let key_count = match &entry.ty {
                        sube::metadata::StorageEntryType::Plain(_) => 0,
                        sube::metadata::StorageEntryType::Map { hashers, key, .. } => {
                            match chain.metadata().registry.resolve(*key) {
                                Some(
                                    sube::scales::TypeDef::Tuple(keys)
                                    | sube::scales::TypeDef::StructTuple(keys),
                                ) => keys.len(),
                                _ => hashers.len(),
                            }
                        }
                    };
                    (format!("{}/{}", pallet.name, entry.name), key_count)
                })
                .collect()
        })
        .unwrap_or_default();
    let pass = format!("0x{}", hex::encode(pass_account));
    let session = format!("0x{}", hex::encode(session_account));
    for (base, key_count) in candidates {
        let path = match key_count {
            0 => base,
            1 => format!("{base}/{session}"),
            _ => format!("{base}/{pass}/{session}"),
        };
        if chain
            .query(&path)
            .await
            .is_ok_and(|response| !response.is_none())
        {
            return true;
        }
    }
    false
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the ignored E2E test"))
}
