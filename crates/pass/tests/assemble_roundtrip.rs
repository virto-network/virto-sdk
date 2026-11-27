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

use std::cell::RefCell;

use pass::workflow::{ChallengeRequest, DeviceAttestation};
use sube::extrinsic::{ChainContext, encode_extensions};
use sube::{DynValue, ExtrinsicAssembler, Metadata, Mortality, Result};

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
/// WebAuthn credential.
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

    let (extra, _additional, _) = result.expect("encode_extensions should succeed");

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
    let (after_extra, after_additional, _) = encode_extensions(
        &meta.extrinsic.extensions[pass_index + 1..],
        &meta.registry,
        &ctx(),
        &[],
    )
    .expect("extensions after PassAuthenticate");
    let implication = pass::inherited_implication(0, &call.bytes, &after_extra, &after_additional);

    // blake2_256 of, written out by hand:
    //   00                     extension version
    //   06 02 11{32}           Pass.remove_device(device_id)
    //   05 00 | 00 | 00 00     CheckMortality, CheckNonce, ChargeAssetTxPayment
    //   64000000 02000000      spec and transaction versions
    //   ab{32} cd{32}          genesis and checkpoint hashes
    assert_eq!(
        hex::encode(implication),
        "4da426ed2064f3e3f7d741ba9a6dbe27f724eeaa26c21b8c2812f844c0fda36c"
    );
}

/// Device that records the challenge it is asked to sign and answers with a
/// fixed WebAuthn assertion built from the request.
struct RecordingDevice {
    challenge: RefCell<Option<[u8; 32]>>,
}

impl pass::DeviceAuthenticator for RecordingDevice {
    fn variant(&self) -> &'static str {
        "WebAuthn"
    }

    async fn attest(&self, _: &ChallengeRequest) -> Result<DeviceAttestation> {
        unreachable!("assembling never attests")
    }

    async fn assert(&self, request: &ChallengeRequest) -> Result<DynValue> {
        *self.challenge.borrow_mut() = Some(request.challenge);
        Ok(DynValue::obj(&[(
            "WebAuthn",
            DynValue::obj(&[
                (
                    "meta",
                    DynValue::obj(&[
                        ("authority_id", DynValue::from(request.authority_id.0)),
                        ("user_id", DynValue::from(request.user_id.0)),
                        ("context", DynValue::from(request.context)),
                    ]),
                ),
                ("authenticator_data", DynValue::Bytes(vec![0x55; 3])),
                ("client_data", DynValue::Bytes(vec![0x66; 2])),
                ("signature", DynValue::Bytes(vec![0x77; 4])),
            ]),
        )]))
    }
}

/// The full V5 assembly path: the device is challenged with
/// `block_challenge(block_hash, inherited_implication)` and its credential
/// lands in the PassAuthenticate extension, followed by the call.
#[test]
fn pass_authenticator_assembles_a_device_credential() {
    let meta = Metadata::from_bytes(KREIVO_META).expect("decode");
    let config = pass::PassRuntimeConfig::discover(&meta).expect("pass config");
    let call = pass::workflow::prepare_remove_device(&meta, &config, pass::DeviceId([0x11; 32]))
        .expect("remove_device call");
    let block_hash = [0x99; 32];
    let device = RecordingDevice {
        challenge: RefCell::new(None),
    };
    let authenticator = pass::PassAuthenticator::new(
        &device,
        pass::Account([0x33; 32]),
        pass::DeviceId([0x02; 32]),
        pass::HashedUserId([0x44; 32]),
        pass::AuthorityId([0x22; 32]),
        42,
        block_hash,
    );
    assert_eq!(authenticator.nonce_account(), [0x33; 32]);

    let assembled = futures_lite::future::block_on(authenticator.assemble(
        &call.bytes,
        &meta.extrinsic,
        &meta.registry,
        &ctx(),
        &[],
    ))
    .expect("assemble");

    let pass_index = meta
        .extrinsic
        .extensions
        .iter()
        .position(|extension| extension.identifier == "PassAuthenticate")
        .expect("PassAuthenticate");
    let (after_extra, after_additional, _) = encode_extensions(
        &meta.extrinsic.extensions[pass_index + 1..],
        &meta.registry,
        &ctx(),
        &[],
    )
    .expect("extensions after PassAuthenticate");
    let implication = pass::inherited_implication(0, &call.bytes, &after_extra, &after_additional);
    assert_eq!(
        *device.challenge.borrow(),
        Some(pass::block_challenge(&block_hash, &implication))
    );

    // General V5 | extension version | PassAuthenticate (first extension) ...
    let mut expected = vec![0x45, 0x00, 0x01];
    expected.extend([0x02; 32]);
    expected.push(0x00); // PassCredential::WebAuthn
    expected.extend([0x22; 32]);
    expected.extend([0x44; 32]);
    expected.extend(42u32.to_le_bytes());
    expected.extend([3 << 2, 0x55, 0x55, 0x55]);
    expected.extend([2 << 2, 0x66, 0x66]);
    expected.extend([4 << 2, 0x77, 0x77, 0x77, 0x77]);
    assert_eq!(
        hex::encode(&assembled.bytes[..expected.len()]),
        hex::encode(&expected)
    );
    // ... the remaining extensions, then the call.
    assert!(assembled.bytes.ends_with(&call.bytes));
    assert_eq!(
        assembled.bytes.len(),
        expected.len() + after_extra.len() + call.bytes.len()
    );
    assert_eq!(assembled.signing_account, vec![0x02; 32]);
    assert_eq!(assembled.scheme, "PassAuthenticate");
}

fn identity() -> pass::PassIdentity {
    pass::PassIdentity {
        account: pass::Account([0x33; 32]),
        device_id: pass::DeviceId([0x02; 32]),
        user_id: pass::HashedUserId([0x44; 32]),
        authority_id: pass::AuthorityId([0x22; 32]),
        context: 42,
        block_hash: [0x99; 32],
    }
}

/// A device that answers later gives the bytes a device at hand gives: the
/// `Pending` asks what `PassAuthenticator` asks, and `finish` adds only the
/// length prefix.
#[test]
fn a_pending_transaction_finishes_as_the_one_step_assembly() {
    let meta = Metadata::from_bytes(KREIVO_META).expect("decode");
    let config = pass::PassRuntimeConfig::discover(&meta).expect("pass config");
    let call = pass::workflow::prepare_remove_device(&meta, &config, pass::DeviceId([0x11; 32]))
        .expect("remove_device call");
    let identity = identity();

    let device = RecordingDevice {
        challenge: RefCell::new(None),
    };
    let authenticator = pass::PassAuthenticator::new(
        &device,
        identity.account,
        identity.device_id,
        identity.user_id,
        identity.authority_id,
        identity.context,
        identity.block_hash,
    );
    assert_eq!(authenticator.identity(), identity);
    let one_step = futures_lite::future::block_on(authenticator.assemble(
        &call.bytes,
        &meta.extrinsic,
        &meta.registry,
        &ctx(),
        &[],
    ))
    .expect("assemble");

    let pending =
        pass::Pending::new(identity, call.clone(), ctx(), Vec::new(), &meta).expect("pending");
    assert_eq!(Some(pending.request.challenge), *device.challenge.borrow());
    // Mortal for 64 blocks from the checkpoint at 128.
    assert_eq!(pending.expires_at(), Some(192));

    // The answer arrives: the same credential the device at hand gave.
    let credential = futures_lite::future::block_on(pass::DeviceAuthenticator::assert(
        &device,
        &pending.request,
    ))
    .expect("credential");
    let extrinsic = pending.finish(credential, &meta).expect("finish");

    let length = u8::try_from(one_step.bytes.len() % 64).expect("fits") << 2 | 0b01;
    // A two-byte compact length: the extrinsic is longer than 63 bytes.
    assert!(one_step.bytes.len() > 63 && one_step.bytes.len() < 1 << 14);
    assert_eq!(extrinsic.bytes[0], length);
    assert_eq!(
        hex::encode(&extrinsic.bytes[2..]),
        hex::encode(&one_step.bytes)
    );
    assert_eq!(extrinsic.call, call);
    assert_eq!(extrinsic.nonce, ctx().account_nonce);
    assert_eq!(extrinsic.expires_at, Some(192));
    assert_eq!(extrinsic.authorization.nonce_account, vec![0x33; 32]);
    assert_eq!(extrinsic.authorization.signing_account, vec![0x02; 32]);
}

/// The answer is for one transaction: a `Pending` whose call or context
/// changed after the device was asked is refused.
#[test]
fn a_pending_transaction_refuses_an_answer_to_another_challenge() {
    let meta = Metadata::from_bytes(KREIVO_META).expect("decode");
    let config = pass::PassRuntimeConfig::discover(&meta).expect("pass config");
    let call = pass::workflow::prepare_remove_device(&meta, &config, pass::DeviceId([0x11; 32]))
        .expect("remove_device call");
    let pending = pass::Pending::new(identity(), call, ctx(), Vec::new(), &meta).expect("pending");
    let credential = pass::key::credential(
        &pending.request,
        pass::key::SignatureType::Sr25519,
        &[9; 64],
    );
    pending
        .finish(credential.clone(), &meta)
        .expect("the unchanged transaction finishes");

    let mut later = pending.clone();
    later.context.account_nonce += 1;
    assert!(later.finish(credential.clone(), &meta).is_err());

    let mut other = pending.clone();
    other.call = pass::workflow::prepare_remove_device(&meta, &config, pass::DeviceId([0x12; 32]))
        .expect("remove_device call");
    assert!(other.finish(credential, &meta).is_err());
}

/// A registration built from an attestation made elsewhere is the call
/// `prepare_enrollment` builds with the device at hand.
#[test]
fn a_registration_from_an_attestation_is_the_enrollment_call() {
    let meta = Metadata::from_bytes(KREIVO_META).expect("decode");
    let config = pass::PassRuntimeConfig::discover(&meta).expect("pass config");
    let user = pass::HashedUserId([0x44; 32]);
    let request = ChallengeRequest::enrollment(
        user,
        config.derive_account(user),
        config.authority_id,
        42,
        &[0x99; 32],
    );
    let attestation = pass::key::attestation(
        &request,
        &[7; 32],
        pass::key::SignatureType::Sr25519,
        &[9; 64],
    );
    assert_eq!(attestation.device_id, pass::key::device_id(&[7; 32]));
    assert_eq!(attestation.device_id, pass::DeviceId([7; 32]));
    let call =
        pass::workflow::prepare_register(&meta, &config, user, &attestation).expect("register");
    assert_eq!(call.pallet, config.pallet);
    assert!(!call.bytes.is_empty());

    let unknown = DeviceAttestation {
        variant: "Unknown".into(),
        ..attestation
    };
    assert!(pass::workflow::prepare_register(&meta, &config, user, &unknown).is_err());
}
