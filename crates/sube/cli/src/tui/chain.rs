use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use super::BlockInfo;
use super::format::format_response;
use sube::{ChainProperties, Metadata};

// --- Messages ---

pub enum ToChain {
    Query(String),
    #[allow(dead_code)]
    QueryAtHash(String, String),
    PrepareCall(String, CallBody),
    SubmitPrepared(sube::WaitFor),
    FetchBlockDetail(String),
    #[cfg(feature = "wallet")]
    ImportWallet {
        name: String,
        mnemonic: zeroize::Zeroizing<String>,
    },
    #[cfg(feature = "wallet")]
    ActivateProfile(String),
    #[cfg(feature = "pass")]
    PreparePassSession {
        profile: String,
        policy: String,
        duration: Option<u32>,
    },
    #[cfg(feature = "pass")]
    ForgetPassSession(String),
}

pub enum CallBody {
    Text(String),
    Json(String),
}

pub enum FromChain {
    Block(BlockInfo),
    Finalized(Vec<String>),
    StorageResult(String),
    StorageError(String),
    CallPrepared(CallReview),
    CallError(String),
    BlockDetail(String, String),
    #[cfg(feature = "wallet")]
    ProfilesUpdated(crate::profiles::Profiles, String),
    #[cfg(feature = "wallet")]
    ProfileError(String),
}

pub struct CallReview {
    pub text: String,
    pub call_hex: Option<String>,
    pub extrinsic_hex: Option<String>,
    pub artifact: Option<String>,
    pub submittable: bool,
    pub requires_finalized: bool,
}

pub struct ChainReady {
    pub metadata: Metadata,
    pub properties: ChainProperties,
    pub genesis_hash: [u8; 32],
}

// --- Chain task ---

pub fn spawn(
    chain_url: String,
    profile_path: PathBuf,
    from_ui: mpsc::Receiver<ToChain>,
    to_ui: smol::channel::Sender<FromChain>,
    ready: mpsc::SyncSender<Result<ChainReady, String>>,
) {
    thread::spawn(move || {
        smol::block_on(async {
            let mut chain = match sube::Sube::connect(&chain_url).await {
                Ok(chain) => chain,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            let metadata = chain.metadata().clone();
            let properties = chain.chain_properties().await.cloned().unwrap_or_default();
            use sube::Backend;
            let genesis_hash = match chain.backend().block_info(Some(0)).await {
                Ok(block) => block.hash,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            if ready
                .send(Ok(ChainReady {
                    metadata,
                    properties,
                    genesis_hash,
                }))
                .is_err()
            {
                return;
            }
            let mut prepared_transaction: Option<crate::workflow::PreparedTransaction> = None;
            let mut prepared_artifact: Option<String> = None;
            #[cfg(feature = "pass")]
            let mut prepared_effect: Option<crate::workflow::PendingEffect> = None;

            loop {
                while let Ok(cmd) = from_ui.try_recv() {
                    match cmd {
                        ToChain::Query(path) => match chain.query(&path).await {
                            Ok(resp) => {
                                let _ = to_ui
                                    .send(FromChain::StorageResult(format_response(resp)))
                                    .await;
                            }
                            Err(e) => {
                                let _ = to_ui.send(FromChain::StorageError(format!("{e}"))).await;
                            }
                        },
                        ToChain::QueryAtHash(path, hash) => {
                            match chain.query_at_hash(&path, &hash).await {
                                Ok(resp) => {
                                    let _ = to_ui
                                        .send(FromChain::StorageResult(format_response(resp)))
                                        .await;
                                }
                                Err(e) => {
                                    let _ =
                                        to_ui.send(FromChain::StorageError(format!("{e}"))).await;
                                }
                            }
                        }
                        ToChain::PrepareCall(path, body) => {
                            prepared_transaction = None;
                            prepared_artifact = None;
                            #[cfg(feature = "pass")]
                            {
                                prepared_effect = None;
                            }
                            let prepared = match body {
                                CallBody::Text(body) => {
                                    chain.prepare_call(&path, &sube::Text(&body))
                                }
                                CallBody::Json(body) => {
                                    serde_json::from_str::<serde_json::Value>(&body)
                                        .map_err(|error| sube::Error::Encode(error.to_string()))
                                        .and_then(|body| chain.prepare_call(&path, &body))
                                }
                            };
                            match prepared {
                                Ok(call) => {
                                    #[cfg(feature = "wallet")]
                                    match prepare_review(
                                        &mut chain,
                                        &chain_url,
                                        &profile_path,
                                        &call,
                                    )
                                    .await
                                    {
                                        Ok(Some((prepared, review, artifact))) => {
                                            let call_hex = prepared.transaction.call.hex.clone();
                                            let extrinsic_hex = prepared.transaction.hex.clone();
                                            prepared_transaction = Some(prepared);
                                            prepared_artifact = Some(artifact.clone());
                                            let _ = to_ui
                                                .send(FromChain::CallPrepared(CallReview {
                                                    text: review,
                                                    call_hex: Some(call_hex),
                                                    extrinsic_hex: Some(extrinsic_hex),
                                                    artifact: Some(artifact),
                                                    submittable: true,
                                                    requires_finalized: false,
                                                }))
                                                .await;
                                        }
                                        Ok(None) => {
                                            let review = prepared_call_review(&call);
                                            let _ = to_ui
                                                .send(FromChain::CallPrepared(CallReview {
                                                    text: review,
                                                    call_hex: Some(call.hex.clone()),
                                                    extrinsic_hex: None,
                                                    artifact: None,
                                                    submittable: false,
                                                    requires_finalized: false,
                                                }))
                                                .await;
                                        }
                                        Err(error) => {
                                            let _ = to_ui.send(FromChain::CallError(error)).await;
                                        }
                                    }
                                    #[cfg(not(feature = "wallet"))]
                                    {
                                        let _ = &profile_path;
                                        let review = prepared_call_review(&call);
                                        let _ = to_ui
                                            .send(FromChain::CallPrepared(CallReview {
                                                text: review,
                                                call_hex: Some(call.hex.clone()),
                                                extrinsic_hex: None,
                                                artifact: None,
                                                submittable: false,
                                                requires_finalized: false,
                                            }))
                                            .await;
                                    }
                                }
                                Err(error) => {
                                    let _ =
                                        to_ui.send(FromChain::CallError(error.to_string())).await;
                                }
                            }
                        }
                        ToChain::SubmitPrepared(wait_for) => {
                            let Some(transaction) = prepared_transaction.take() else {
                                let _ = to_ui
                                    .send(FromChain::CallError(
                                        "No reviewed signed transaction is ready".into(),
                                    ))
                                    .await;
                                continue;
                            };
                            #[cfg(feature = "pass")]
                            let effect = prepared_effect.take();
                            match crate::workflow::submit_transaction(
                                &mut chain,
                                &transaction,
                                wait_for,
                            )
                            .await
                            {
                                Ok(receipt) => {
                                    prepared_artifact = None;
                                    #[cfg(feature = "pass")]
                                    if let Some(effect) = effect {
                                        match crate::workflow::apply_finalized_effect(
                                            &profile_path,
                                            &receipt,
                                            effect,
                                        ) {
                                            Ok(profiles) => {
                                                let _ = to_ui
                                                    .send(FromChain::ProfilesUpdated(
                                                        profiles,
                                                        "Finalized session persisted and profile connected."
                                                            .into(),
                                                    ))
                                                    .await;
                                            }
                                            Err(error) => {
                                                let _ = to_ui
                                                    .send(FromChain::ProfileError(
                                                        error.to_string(),
                                                    ))
                                                    .await;
                                            }
                                        }
                                    }
                                    let text = serde_json::to_string_pretty(&serde_json::json!({
                                        "finalizedBlockHash": receipt.finalized_block_hash,
                                        "extrinsicIndex": receipt.extrinsic_index,
                                        "dispatchOutcome": format!("{:?}", receipt.dispatch_outcome),
                                        "events": receipt.events.iter().map(|event| {
                                            serde_json::json!({
                                                "pallet": event.pallet,
                                                "variant": event.variant,
                                                "decoded": event.decoded,
                                            })
                                        }).collect::<Vec<_>>(),
                                    }))
                                    .unwrap_or_else(|_| "transaction finalized".into());
                                    let _ = to_ui
                                        .send(FromChain::CallPrepared(CallReview {
                                            text,
                                            call_hex: None,
                                            extrinsic_hex: None,
                                            artifact: None,
                                            submittable: false,
                                            requires_finalized: false,
                                        }))
                                        .await;
                                }
                                Err(error) => {
                                    prepared_transaction = Some(transaction);
                                    #[cfg(feature = "pass")]
                                    {
                                        prepared_effect = effect;
                                    }
                                    let _ = to_ui
                                        .send(FromChain::CallPrepared(CallReview {
                                            text: format!(
                                                "Submission failed; reviewed bytes retained: {error}\n\nPress s to retry."
                                            ),
                                            call_hex: prepared_transaction
                                                .as_ref()
                                                .map(|prepared| prepared.transaction.call.hex.clone()),
                                            extrinsic_hex: prepared_transaction
                                                .as_ref()
                                                .map(|prepared| prepared.transaction.hex.clone()),
                                            artifact: prepared_artifact.clone(),
                                            submittable: true,
                                            requires_finalized: false,
                                        }))
                                        .await;
                                }
                            }
                        }
                        ToChain::FetchBlockDetail(hash) => {
                            let events = match chain.query_at_hash("system/events", &hash).await {
                                Ok(resp) => format_response(resp),
                                Err(e) => format!("error: {e}"),
                            };
                            let _ = to_ui.send(FromChain::BlockDetail(hash, events)).await;
                        }
                        #[cfg(feature = "wallet")]
                        ToChain::ImportWallet { name, mnemonic } => {
                            match crate::workflow::import_wallet_profile(
                                &profile_path,
                                genesis_hash,
                                name,
                                &mnemonic,
                            ) {
                                Ok(profiles) => {
                                    let _ = to_ui
                                        .send(FromChain::ProfilesUpdated(
                                            profiles,
                                            "Wallet profile imported; select it to connect.".into(),
                                        ))
                                        .await;
                                }
                                Err(error) => {
                                    let _ = to_ui
                                        .send(FromChain::ProfileError(error.to_string()))
                                        .await;
                                }
                            }
                        }
                        #[cfg(feature = "wallet")]
                        ToChain::ActivateProfile(name) => {
                            match crate::workflow::activate_existing_profile(
                                &mut chain,
                                &profile_path,
                                genesis_hash,
                                &name,
                            )
                            .await
                            {
                                Ok(profiles) => {
                                    let _ = to_ui
                                        .send(FromChain::ProfilesUpdated(
                                            profiles,
                                            format!("Connected profile {name}."),
                                        ))
                                        .await;
                                }
                                Err(error) => {
                                    let _ = to_ui
                                        .send(FromChain::ProfileError(error.to_string()))
                                        .await;
                                }
                            }
                        }
                        #[cfg(feature = "pass")]
                        ToChain::PreparePassSession {
                            profile,
                            policy,
                            duration,
                        } => {
                            prepared_transaction = None;
                            prepared_artifact = None;
                            prepared_effect = None;
                            match crate::workflow::prepare_pass_session(
                                &mut chain,
                                &chain_url,
                                &profile_path,
                                genesis_hash,
                                &profile,
                                &policy,
                                duration,
                            )
                            .await
                            {
                                Ok(crate::workflow::SessionPreparation::Reused(profiles)) => {
                                    let _ = to_ui
                                        .send(FromChain::ProfilesUpdated(
                                            profiles,
                                            format!("Reused exact session for {profile}."),
                                        ))
                                        .await;
                                }
                                Ok(crate::workflow::SessionPreparation::Review {
                                    prepared,
                                    effect,
                                }) => {
                                    let prepared = *prepared;
                                    let call_hex = prepared.transaction.call.hex.clone();
                                    let extrinsic_hex = prepared.transaction.hex.clone();
                                    let artifact =
                                        serde_json::to_string_pretty(&prepared.artifact())
                                            .unwrap_or_else(|_| "{}".into());
                                    let mut review = prepared.review_text();
                                    review.push_str(
                                        "\nEsc Back | c Copy call | x Copy extrinsic | e Export JSON | s Submit",
                                    );
                                    prepared_artifact = Some(artifact.clone());
                                    prepared_effect = Some(*effect);
                                    prepared_transaction = Some(prepared);
                                    let _ = to_ui
                                        .send(FromChain::CallPrepared(CallReview {
                                            text: review,
                                            call_hex: Some(call_hex),
                                            extrinsic_hex: Some(extrinsic_hex),
                                            artifact: Some(artifact),
                                            submittable: true,
                                            requires_finalized: true,
                                        }))
                                        .await;
                                }
                                Err(error) => {
                                    let _ = to_ui
                                        .send(FromChain::ProfileError(error.to_string()))
                                        .await;
                                }
                            }
                        }
                        #[cfg(feature = "pass")]
                        ToChain::ForgetPassSession(profile) => {
                            match crate::workflow::forget_pass_session(
                                &profile_path,
                                genesis_hash,
                                &profile,
                            ) {
                                Ok(profiles) => {
                                    let _ = to_ui
                                        .send(FromChain::ProfilesUpdated(
                                            profiles,
                                            pass::session::FORGET_SESSION_WARNING.into(),
                                        ))
                                        .await;
                                }
                                Err(error) => {
                                    let _ = to_ui
                                        .send(FromChain::ProfileError(error.to_string()))
                                        .await;
                                }
                            }
                        }
                    }
                }

                match chain.next_event().await {
                    Ok(sube::ChainEvent::NewBlock { hash, .. }) => {
                        let number = chain.header(&hash).await.map(|h| h.number).unwrap_or(0);
                        // Count events by rough text-scan heuristic; precise
                        // introspection would need to walk scales::Value.
                        let (event_count, has_extrinsics) =
                            match chain.query_at_hash("system/events", &hash).await {
                                Ok(sube::Response::Value(entry, meta)) => {
                                    let text = entry.to_text(&meta.registry).unwrap_or_default();
                                    let total = text.matches("phase").count();
                                    let interesting = text.matches("ApplyExtrinsic").count() > 0;
                                    (total, interesting)
                                }
                                _ => (0, false),
                            };
                        let _ = to_ui
                            .send(FromChain::Block(BlockInfo {
                                number,
                                hash,
                                finalized: false,
                                has_extrinsics,
                                event_count,
                            }))
                            .await;
                    }
                    Ok(sube::ChainEvent::Finalized { hashes, .. }) => {
                        let _ = to_ui.send(FromChain::Finalized(hashes)).await;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        smol::Timer::after(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
        })
    });
}

fn prepared_call_review(call: &sube::PreparedCall) -> String {
    format!(
        "Prepared call (not submitted)\n\n{}::{}\n{}",
        call.pallet, call.call, call.hex
    )
}

#[cfg(feature = "wallet")]
async fn prepare_review(
    chain: &mut sube::Sube,
    chain_url: &str,
    profile_path: &std::path::Path,
    call: &sube::PreparedCall,
) -> Result<Option<(crate::workflow::PreparedTransaction, String, String)>, String> {
    let profiles =
        crate::profiles::Profiles::load(profile_path).map_err(|error| error.to_string())?;
    let Some(profile) = profiles.active() else {
        return Ok(None);
    };
    let genesis_hash = crate::chain_genesis(chain)
        .await
        .map_err(|error| error.to_string())?;
    if profile.genesis_hash() != genesis_hash {
        return Err("active profile genesis hash does not match the connected chain".into());
    }

    let (prepared, authorizer) = match profile {
        crate::profiles::Profile::Wallet(profile) => {
            let signer = crate::wallet_signer(profile)
                .await
                .map_err(|error| error.to_string())?;
            let prepared = crate::workflow::prepare_transaction(
                chain,
                chain_url,
                call,
                &profile.name,
                &signer,
                sube::TransactionOptions::default(),
            )
            .await
            .map_err(|error| error.to_string())?;
            (prepared, profile.name.as_str())
        }
        #[cfg(feature = "pass")]
        crate::profiles::Profile::Pass(profile) => {
            let session = profile
                .session
                .as_ref()
                .ok_or_else(|| "active pass profile has no finalized local session".to_string())?;
            let policy = crate::parse_session_policy(chain.metadata(), &session.policy)
                .map_err(|error| error.to_string())?;
            let signer = crate::load_session_signer(&session.secure_entry, Some(session.account))
                .await
                .map_err(|error| error.to_string())?;
            let authorizer =
                pass::SessionAuthorizer::new(pass::Account(profile.pass_account), signer, policy);
            let prepared = crate::workflow::prepare_transaction(
                chain,
                chain_url,
                call,
                &profile.name,
                &authorizer,
                sube::TransactionOptions::default(),
            )
            .await
            .map_err(|error| error.to_string())?;
            (prepared, profile.name.as_str())
        }
    };
    let _ = authorizer;
    let artifact =
        serde_json::to_string_pretty(&prepared.artifact()).map_err(|error| error.to_string())?;
    let mut review = prepared.review_text();
    review.push_str(
        "\n\nEsc Back | c Copy call | x Copy extrinsic | e Export JSON | s Submit finalized",
    );
    Ok(Some((prepared, review, artifact)))
}

#[cfg(test)]
mod tests {
    use super::Metadata;

    #[test]
    fn owned_metadata_snapshot_can_cross_threads() {
        fn assert_send<T: Send>() {}
        assert_send::<Metadata>();
    }
}
