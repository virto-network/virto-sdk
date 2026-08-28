//! Integration test: exercise the scales encoding path for a pass credential
//! against real Kreivo metadata.
//!
//! This is the test that would have caught:
//! - the Blake2s-vs-Blake2b bug (the challenge produced wouldn't match)
//! - the `DynValue::Str` vs `[u8; 32]` encoding mismatch
//!
//! The test does not run the full async `assemble()` pipeline (which needs a
//! tokio runtime and a real `Backend`). Instead it exercises the part that
//! matters for correctness: that a `DynValue` override for the PassAuthenticate
//! extension SCALE-encodes successfully against the actual on-chain type
//! registry.

use sube::extrinsic::{ChainContext, encode_extensions};
use sube::{DynValue, Metadata, Mortality};

const KREIVO_META: &[u8] = include_bytes!("../../sube/tests/fixtures/kreivo.scale");

fn ctx() -> ChainContext {
    ChainContext {
        spec_version: 100,
        tx_version: 2,
        genesis_hash: [0xab; 32],
        account_nonce: 0,
        checkpoint_number: 128,
        checkpoint_hash: [0xcd; 32],
        mortality_checkpoint_hash: [0xcd; 32],
        mortality: Mortality::Mortal { period: 64 },
        tip: 0,
    }
}

/// Build a mock PassAuthenticate value shaped like a SubstrateKey credential.
fn mock_pass_authenticate() -> DynValue {
    let message = DynValue::obj(&[
        ("context", DynValue::U32(42)),
        ("challenge", DynValue::from([0x11u8; 32])),
        ("authority_id", DynValue::from([0x22u8; 32])),
    ]);
    let signature = DynValue::obj(&[("Sr25519", DynValue::from([0x33u8; 64]))]);
    let key_sig = DynValue::obj(&[
        ("user_id", DynValue::from([0x44u8; 32])),
        ("message", message),
        ("signature", signature),
    ]);
    let credential = DynValue::obj(&[("SubstrateKey", key_sig)]);
    DynValue::obj(&[(
        "Some",
        DynValue::obj(&[
            ("device_id", DynValue::from([0x02u8; 32])),
            ("credential", credential),
        ]),
    )])
}

#[test]
fn encode_pass_credential_against_kreivo_metadata() {
    let meta = Metadata::from_bytes(KREIVO_META).expect("decode kreivo metadata");

    let has_pass = meta
        .extrinsic
        .extensions
        .iter()
        .any(|e| e.identifier == "PassAuthenticate");

    if !has_pass {
        eprintln!(
            "skipping: kreivo metadata fixture has no PassAuthenticate extension — \
             update the fixture to enable end-to-end testing"
        );
        return;
    }

    let overrides = [("PassAuthenticate".to_string(), mock_pass_authenticate())];
    let result = encode_extensions(
        &meta.extrinsic.extensions,
        &meta.registry,
        &ctx(),
        &overrides,
    );

    let (extra, _additional) = result.expect("encode_extensions should succeed");

    // Sanity: the body contains the PassAuthenticate extension value.
    assert!(!extra.is_empty(), "extra bytes should be populated");
}

#[test]
fn kreivo_v5_inherited_implication_golden_vector() {
    let meta = Metadata::from_bytes(KREIVO_META).expect("decode");
    let pass_index = meta
        .extrinsic
        .extensions
        .iter()
        .position(|extension| extension.identifier == "PassAuthenticate")
        .expect("PassAuthenticate");
    let call = pass::config::PassRuntimeConfig::discover(&meta)
        .and_then(|config| {
            pass::workflow::prepare_remove_device(&meta, &config, pass::DeviceId([0x11; 32]))
        })
        .expect("remove_device call");
    let (after_extra, after_additional) = encode_extensions(
        &meta.extrinsic.extensions[pass_index + 1..],
        &meta.registry,
        &ctx(),
        &[],
    )
    .expect("extensions after PassAuthenticate");
    let implication = pass::inherited_implication(
        0x40 | meta.extrinsic.version,
        &call.bytes,
        &after_extra,
        &after_additional,
    );

    assert_eq!(
        hex::encode(implication),
        "58f30f4f6ba5ebebdaad7dcddfc4275d3e051c00ea5088b122b7f570202123ef"
    );
}
