//! Integration tests against a live Substrate chain.
//!
//! These tests require network access and are ignored by default.
//! Run with: cargo test --features test --test integration -- --ignored

use sube::{Response, Sube};

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
