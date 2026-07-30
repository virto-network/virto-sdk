use std::sync::mpsc;
use std::thread;

use super::BlockInfo;
use super::format::format_response;
use sube::Metadata;

// --- Messages ---

pub enum ToChain {
    Query(String),
    #[allow(dead_code)]
    QueryAtHash(String, String),
    FetchBlockDetail(String),
}

pub enum FromChain {
    Block(BlockInfo),
    Finalized(Vec<String>),
    StorageResult(String),
    StorageError(String),
    BlockDetail(String, String),
}

// --- Chain task ---

pub fn spawn(
    chain_url: String,
    from_ui: mpsc::Receiver<ToChain>,
    to_ui: smol::channel::Sender<FromChain>,
    ready: mpsc::SyncSender<Result<Metadata, String>>,
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
            if ready.send(Ok(chain.metadata().clone())).is_err() {
                return;
            }

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
                        ToChain::FetchBlockDetail(hash) => {
                            let events = match chain.query_at_hash("system/events", &hash).await {
                                Ok(resp) => format_response(resp),
                                Err(e) => format!("error: {e}"),
                            };
                            let _ = to_ui.send(FromChain::BlockDetail(hash, events)).await;
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

#[cfg(test)]
mod tests {
    use super::Metadata;

    #[test]
    fn owned_metadata_snapshot_can_cross_threads() {
        fn assert_send<T: Send>() {}
        assert_send::<Metadata>();
    }
}
