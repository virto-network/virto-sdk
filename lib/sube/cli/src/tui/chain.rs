use std::sync::mpsc;
use std::thread;

use super::format::{format_events_detail, format_response, is_interesting_event};
use super::BlockInfo;

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
    mut chain: sube::Sube,
    from_ui: mpsc::Receiver<ToChain>,
    to_ui: smol::channel::Sender<FromChain>,
) {
    thread::spawn(move || {
        smol::block_on(async {
            loop {
                while let Ok(cmd) = from_ui.try_recv() {
                    match cmd {
                        ToChain::Query(path) => {
                            match chain.query(&path).await {
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
                            let events =
                                match chain.query_at_hash("system/events", &hash).await {
                                    Ok(resp) => match resp.to_json() {
                                        Ok(Some(json)) => format_events_detail(&json),
                                        Ok(None) => "(no events)".into(),
                                        Err(e) => format!("decode error: {e}"),
                                    },
                                    Err(e) => format!("error: {e}"),
                                };
                            let _ = to_ui.send(FromChain::BlockDetail(hash, events)).await;
                        }
                    }
                }

                match chain.next_event().await {
                    Ok(sube::ChainEvent::NewBlock { hash, .. }) => {
                        let number =
                            chain.header(&hash).await.map(|h| h.number).unwrap_or(0);
                        let (event_count, interesting) =
                            match chain.query_at_hash("system/events", &hash).await {
                                Ok(sube::Response::Value(entry, meta)) => {
                                    match entry.to_json(&meta.registry) {
                                        Ok(json) => {
                                            let events = json.as_array();
                                            let total =
                                                events.map(|a| a.len()).unwrap_or(0);
                                            let interesting = events
                                                .map(|arr| {
                                                    arr.iter()
                                                        .filter(|e| is_interesting_event(e))
                                                        .count()
                                                })
                                                .unwrap_or(0);
                                            (total, interesting)
                                        }
                                        Err(_) => (0, 0),
                                    }
                                }
                                _ => (0, 0),
                            };
                        let _ = to_ui
                            .send(FromChain::Block(BlockInfo {
                                number,
                                hash,
                                finalized: false,
                                has_extrinsics: interesting > 0,
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
