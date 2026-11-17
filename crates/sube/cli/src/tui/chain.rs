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
    /// Prepare `pallet/call` from its SCALE-text body.
    PrepareCall(String, String),
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
    #[cfg(feature = "pass")]
    PrepareEnrollment(crate::workflow::EnrollmentRequest),
    #[cfg(feature = "pass")]
    PrepareDeviceAddition(crate::workflow::DeviceAdditionRequest),
    #[cfg(feature = "pass")]
    PrepareDeviceRemoval(crate::workflow::DeviceRemovalRequest),
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
            let properties = chain.chain_properties().await.unwrap_or_default();
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
            let mut staged: Option<crate::workflow::Staged> = None;
            #[cfg(not(feature = "wallet"))]
            let _ = &profile_path;

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
                            staged = None;
                            let call = match chain.prepare_call(&path, &sube::Text(&body)) {
                                Ok(call) => call,
                                Err(error) => {
                                    let _ =
                                        to_ui.send(FromChain::CallError(error.to_string())).await;
                                    continue;
                                }
                            };
                            #[cfg(feature = "wallet")]
                            let signed = crate::workflow::prepare_with_active_profile(
                                &mut chain,
                                &chain_url,
                                &profile_path,
                                &call,
                            )
                            .await;
                            #[cfg(not(feature = "wallet"))]
                            let signed = anyhow::Result::<
                                Option<crate::workflow::PreparedTransaction>,
                            >::Ok(None);
                            match signed {
                                Ok(Some(prepared)) => {
                                    let plain = crate::workflow::Staged {
                                        prepared,
                                        notes: Vec::new(),
                                        effect: None,
                                    };
                                    stage(&to_ui, &mut staged, plain).await;
                                }
                                Ok(None) => {
                                    let _ = to_ui
                                        .send(FromChain::CallPrepared(CallReview {
                                            text: prepared_call_review(&call),
                                            submittable: false,
                                            requires_finalized: false,
                                        }))
                                        .await;
                                }
                                Err(error) => {
                                    let _ =
                                        to_ui.send(FromChain::CallError(error.to_string())).await;
                                }
                            }
                        }
                        ToChain::SubmitPrepared(wait_for) => {
                            let Some(current) = staged.take() else {
                                let _ = to_ui
                                    .send(FromChain::CallError(
                                        "No reviewed signed transaction is ready".into(),
                                    ))
                                    .await;
                                continue;
                            };
                            match crate::workflow::submit_transaction(
                                &mut chain,
                                &current.prepared,
                                wait_for,
                            )
                            .await
                            {
                                Ok(receipt) => {
                                    #[cfg(feature = "pass")]
                                    if let Some(effect) = current.effect {
                                        let message = effect.finalized_message();
                                        let update = match crate::workflow::apply_finalized_effect(
                                            &profile_path,
                                            &receipt,
                                            effect,
                                        ) {
                                            Ok(profiles) => {
                                                FromChain::ProfilesUpdated(profiles, message.into())
                                            }
                                            Err(error) => {
                                                FromChain::ProfileError(error.to_string())
                                            }
                                        };
                                        let _ = to_ui.send(update).await;
                                    }
                                    let text = serde_json::to_string_pretty(
                                        &crate::workflow::receipt_json(&receipt),
                                    )
                                    .unwrap_or_else(|_| "transaction finalized".into());
                                    let _ = to_ui
                                        .send(FromChain::CallPrepared(CallReview {
                                            text,
                                            submittable: false,
                                            requires_finalized: false,
                                        }))
                                        .await;
                                }
                                Err(error) => {
                                    let mut review = review_of(&current);
                                    review.text = format!(
                                        "Submission failed; reviewed bytes retained: {error}\n\nPress s to retry."
                                    );
                                    staged = Some(current);
                                    let _ = to_ui.send(FromChain::CallPrepared(review)).await;
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
                            let update = match crate::workflow::import_wallet_profile(
                                &profile_path,
                                genesis_hash,
                                name,
                                &mnemonic,
                            ) {
                                Ok(profiles) => FromChain::ProfilesUpdated(
                                    profiles,
                                    "Wallet profile imported; select it to connect.".into(),
                                ),
                                Err(error) => FromChain::ProfileError(error.to_string()),
                            };
                            let _ = to_ui.send(update).await;
                        }
                        #[cfg(feature = "wallet")]
                        ToChain::ActivateProfile(name) => {
                            let update = match crate::workflow::activate_existing_profile(
                                &mut chain,
                                &profile_path,
                                genesis_hash,
                                &name,
                            )
                            .await
                            {
                                Ok(profiles) => FromChain::ProfilesUpdated(
                                    profiles,
                                    format!("Connected profile {name}."),
                                ),
                                Err(error) => FromChain::ProfileError(error.to_string()),
                            };
                            let _ = to_ui.send(update).await;
                        }
                        #[cfg(feature = "pass")]
                        ToChain::PreparePassSession {
                            profile,
                            policy,
                            duration,
                        } => {
                            staged = None;
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
                                Ok(crate::workflow::SessionPreparation::Review(review)) => {
                                    stage(&to_ui, &mut staged, *review).await;
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
                            let update = match crate::workflow::forget_pass_session(
                                &profile_path,
                                Some(genesis_hash),
                                &profile,
                            ) {
                                Ok(profiles) => FromChain::ProfilesUpdated(
                                    profiles,
                                    pass::session::FORGET_SESSION_WARNING.into(),
                                ),
                                Err(error) => FromChain::ProfileError(error.to_string()),
                            };
                            let _ = to_ui.send(update).await;
                        }
                        #[cfg(feature = "pass")]
                        ToChain::PrepareEnrollment(request) => {
                            staged = None;
                            let result = crate::workflow::prepare_enrollment(
                                &mut chain,
                                &chain_url,
                                &profile_path,
                                genesis_hash,
                                request,
                            )
                            .await;
                            stage_pass(&to_ui, &mut staged, result).await;
                        }
                        #[cfg(feature = "pass")]
                        ToChain::PrepareDeviceAddition(request) => {
                            staged = None;
                            let result = crate::workflow::prepare_device_addition(
                                &mut chain,
                                &chain_url,
                                &profile_path,
                                genesis_hash,
                                request,
                            )
                            .await;
                            stage_pass(&to_ui, &mut staged, result).await;
                        }
                        #[cfg(feature = "pass")]
                        ToChain::PrepareDeviceRemoval(request) => {
                            staged = None;
                            let result = crate::workflow::prepare_device_removal(
                                &mut chain,
                                &chain_url,
                                &profile_path,
                                genesis_hash,
                                request,
                            )
                            .await;
                            stage_pass(&to_ui, &mut staged, result).await;
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
        call.pallet,
        call.call,
        call.hex()
    )
}

/// The review screen of a signed transaction. Transactions that change the
/// local profile must finalize before that change is applied.
fn review_of(staged: &crate::workflow::Staged) -> CallReview {
    CallReview {
        text: staged.review_text(),
        submittable: true,
        requires_finalized: staged.effect.is_some(),
    }
}

/// Show the review of `staged` and keep it for submission.
async fn stage(
    to_ui: &smol::channel::Sender<FromChain>,
    slot: &mut Option<crate::workflow::Staged>,
    staged: crate::workflow::Staged,
) {
    let review = review_of(&staged);
    *slot = Some(staged);
    let _ = to_ui.send(FromChain::CallPrepared(review)).await;
}

/// Stage a prepared pass operation, or report why it could not be prepared.
#[cfg(feature = "pass")]
async fn stage_pass(
    to_ui: &smol::channel::Sender<FromChain>,
    slot: &mut Option<crate::workflow::Staged>,
    result: anyhow::Result<crate::workflow::Staged>,
) {
    match result {
        Ok(staged) => stage(to_ui, slot, staged).await,
        Err(error) => {
            let _ = to_ui.send(FromChain::ProfileError(error.to_string())).await;
        }
    }
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
