//! Integration tests against a live Substrate chain.
//!
//! These tests require network access and are ignored by default.
//! Run with: cargo test --features test --test integration -- --ignored

use sube::{ChainEvent, Response, Sube};

const CHAIN: &str = "wss://kreivo.io";
const ADDR: &str = "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b";

fn block_on<T>(fut: impl core::future::Future<Output = T>) -> T {
    smol::block_on(fut)
}

#[test]
#[ignore]
fn connect_and_get_metadata() {
    block_on(async {
        let chain = Sube::connect(CHAIN).await.expect("connects");
        let meta = chain.metadata();
        assert!(!meta.pallets.is_empty(), "has pallets");
        let system = meta.pallet_by_name("System");
        assert!(system.is_some(), "System pallet exists");
    });
}

#[test]
#[ignore]
fn query_system_account() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");
        let response = chain
            .query(&format!("system/account/{ADDR}"))
            .await
            .expect("queries");
        match response {
            Response::Value(entry, meta) => {
                let json = entry.to_json(&meta.registry).expect("decodes");
                assert!(json.get("nonce").is_some(), "has nonce field");
                assert!(json.get("data").is_some(), "has data field");
            }
            other => panic!("expected Value, got {other:?}"),
        }
    });
}

#[test]
#[ignore]
fn query_constant() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");
        let response = chain
            .query("system/_constants/Version")
            .await
            .expect("queries constant");
        match response {
            Response::Value(entry, meta) => {
                let json = entry.to_json(&meta.registry).expect("decodes");
                assert!(json.get("spec_name").is_some(), "has spec_name");
                assert!(json.get("spec_version").is_some(), "has spec_version");
            }
            other => panic!("expected Value, got {other:?}"),
        }
    });
}

#[test]
#[ignore]
fn query_storage_map_iteration() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");
        let response = chain
            .query("communityMemberships/collection")
            .await
            .expect("queries map");
        match response {
            Response::ValueSet(entries, _meta) => {
                assert!(!entries.is_empty(), "has entries");
            }
            other => panic!("expected ValueSet, got {other:?}"),
        }
    });
}

#[test]
#[ignore]
fn query_meta_path() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");
        let response = chain.query("_meta").await.expect("queries _meta");
        match response {
            Response::Meta(meta) => {
                assert!(!meta.pallets.is_empty(), "metadata has pallets");
            }
            other => panic!("expected Meta, got {other:?}"),
        }
    });
}

#[test]
#[ignore]
fn one_shot_query() {
    block_on(async {
        let response = sube::sube(&format!("{CHAIN}/system/account/{ADDR}"))
            .await
            .expect("one-shot query");
        match response {
            Response::Value(entry, meta) => {
                let json = entry.to_json(&meta.registry).expect("decodes");
                assert!(json.get("nonce").is_some());
            }
            other => panic!("expected Value, got {other:?}"),
        }
    });
}

#[test]
#[ignore]
fn text_format_output() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");
        let response = chain
            .query("system/_constants/Version")
            .await
            .expect("queries");
        if let Response::Value(entry, meta) = response {
            let text = entry.to_text(&meta.registry).expect("text format works");
            assert!(!text.is_empty(), "text output is non-empty");
        }
    });
}

#[test]
#[ignore]
fn follow_chain_events() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");

        // Receive a few chain events
        let mut saw_new_block = false;
        let mut saw_finalized = false;
        for _ in 0..100 {
            let event = chain.next_event().await.expect("gets event");
            match event {
                ChainEvent::NewBlock {
                    hash, parent, ..
                } => {
                    assert!(hash.starts_with("0x"), "hash is hex");
                    assert!(parent.starts_with("0x"), "parent is hex");
                    saw_new_block = true;
                }
                ChainEvent::Finalized { hashes, .. } => {
                    assert!(!hashes.is_empty(), "has finalized hashes");
                    saw_finalized = true;
                }
                ChainEvent::BestBlock { hash } => {
                    assert!(hash.starts_with("0x"), "best hash is hex");
                }
            }
            if saw_new_block && saw_finalized {
                break;
            }
        }
        assert!(saw_new_block, "saw at least one NewBlock event");
        assert!(saw_finalized, "saw at least one Finalized event");
    });
}

#[test]
#[ignore]
fn query_at_new_block() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");

        // Wait for a new block, then query at that block hash
        loop {
            match chain.next_event().await.expect("gets event") {
                ChainEvent::NewBlock { ref hash, .. } => {
                    let response = chain
                        .query_at_hash(&format!("system/account/{ADDR}"), hash)
                        .await
                        .expect("queries at block hash");
                    match response {
                        Response::Value(entry, meta) => {
                            let json = entry.to_json(&meta.registry).expect("decodes");
                            assert!(json.get("nonce").is_some(), "has nonce");
                        }
                        Response::None => {} // account may not exist at this block
                        other => panic!("expected Value or None, got {other:?}"),
                    }
                    break;
                }
                _ => continue,
            }
        }
    });
}

#[test]
#[ignore]
fn decode_block_events() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");

        // Wait for a new block and decode its events
        loop {
            match chain.next_event().await.expect("gets event") {
                ChainEvent::NewBlock {
                    ref hash, number, ..
                } => {
                    // Query System::Events at this block
                    let response = chain
                        .query_at_hash("system/events", hash)
                        .await
                        .expect("queries events");

                    match response {
                        Response::Value(entry, meta) => {
                            let json = entry.to_json(&meta.registry).expect("decodes events");
                            let events = json.as_array().expect("events is an array");
                            assert!(!events.is_empty(), "block {number} has events");

                            // Each event has phase and event fields
                            let first = &events[0];
                            assert!(
                                first.get("phase").is_some(),
                                "event has phase"
                            );
                            assert!(
                                first.get("event").is_some(),
                                "event has event body"
                            );
                        }
                        other => panic!("expected Value, got {other:?}"),
                    }
                    break;
                }
                _ => continue,
            }
        }
    });
}

#[test]
#[ignore]
fn fetch_block_header() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");

        // Wait for a new block and fetch its full header
        loop {
            match chain.next_event().await.expect("gets event") {
                ChainEvent::NewBlock {
                    ref hash, number, ..
                } => {
                    let header = chain.header(hash).await.expect("gets header");
                    assert_eq!(header.number, number, "header number matches event");
                    assert!(
                        header.state_root.starts_with("0x"),
                        "state_root is hex"
                    );
                    assert!(
                        header.extrinsics_root.starts_with("0x"),
                        "extrinsics_root is hex"
                    );
                    break;
                }
                _ => continue,
            }
        }
    });
}

#[test]
#[ignore]
fn query_after_finalization() {
    block_on(async {
        let mut chain = Sube::connect(CHAIN).await.expect("connects");

        // Wait for a finalization, then query storage
        chain.next_finalized().await.expect("gets finalized");
        let response = chain
            .query(&format!("system/account/{ADDR}"))
            .await
            .expect("queries after finalization");
        match response {
            Response::Value(entry, meta) => {
                let json = entry.to_json(&meta.registry).expect("decodes");
                assert!(json.get("nonce").is_some(), "has nonce");
            }
            other => panic!("expected Value, got {other:?}"),
        }
    });
}
