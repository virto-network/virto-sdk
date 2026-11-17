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
        mortality: Mortality::Mortal { period: 64 },
        tip: 0,
    }
}

/// Build a mock PassAuthenticate value shaped like the fixture runtime's
/// WebAuthn credential (the fixture predates `SubstrateKey`).
fn mock_pass_authenticate() -> DynValue {
    let meta = DynValue::obj(&[
        ("authority_id", DynValue::from([0x22u8; 32])),
        ("user_id", DynValue::from([0x44u8; 32])),
        ("context", DynValue::U32(42)),
    ]);
    let credential = DynValue::obj(&[(
        "WebAuthn",
        DynValue::obj(&[
            ("meta", meta),
            ("authenticator_data", DynValue::Bytes(vec![0x55; 3])),
            ("client_data", DynValue::Bytes(vec![0x66; 2])),
            ("signature", DynValue::Bytes(vec![0x77; 4])),
        ]),
    )]);
    // A bare value for the `Option` is encoded as `Some`.
    DynValue::obj(&[
        ("device_id", DynValue::from([0x02u8; 32])),
        ("credential", credential),
    ])
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

    // PassAuthenticate is the first extension: Some | device_id |
    // PassCredential::WebAuthn | meta | compact-prefixed byte fields.
    let mut expected = vec![0x01];
    expected.extend([0x02; 32]);
    expected.push(0x00);
    expected.extend([0x22; 32]);
    expected.extend([0x44; 32]);
    expected.extend(42u32.to_le_bytes());
    expected.extend([3 << 2, 0x55, 0x55, 0x55]);
    expected.extend([2 << 2, 0x66, 0x66]);
    expected.extend([4 << 2, 0x77, 0x77, 0x77, 0x77]);
    assert_eq!(
        hex::encode(&extra[..expected.len()]),
        hex::encode(&expected)
    );
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
        "93480035c8de934618faa5bde6a36f593fc3f5c422a5e12694d572628a84e61d"
    );
}
