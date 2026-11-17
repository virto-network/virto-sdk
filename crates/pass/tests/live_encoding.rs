//! Read-only live checks of pass call and extension encodings against a current
//! pallet-pass runtime (Kreivo by default; override with `SUBE_E2E_URL`).
//!
//! cargo test -p pass --test live_encoding -- --ignored

use pass::session::{CallIndex, prepare_add_session_key};
use pass::{PassRuntimeConfig, SessionPolicy};

/// Scoped sessions must go through `add_session_key_with_filter`: the plain
/// call takes no filter, and pallet-pass grants it an Admin session key.
#[test]
#[ignore = "requires network access to a chain with pallet-pass"]
fn session_call_carries_the_filter_and_an_optional_duration() {
    futures_lite::future::block_on(async {
        let url = std::env::var("SUBE_E2E_URL").unwrap_or_else(|_| "wss://kreivo.io".into());
        let chain = sube::Sube::connect(&url).await.expect("connect");
        let metadata = chain.metadata();
        let config = PassRuntimeConfig::discover(metadata).expect("pass config");
        assert_eq!(config.add_session_key.name, "add_session_key_with_filter");

        let policy = SessionPolicy::Calls(vec![
            CallIndex {
                pallet: 10,
                call: 3,
            },
            CallIndex { pallet: 5, call: 0 },
        ]);
        let call = prepare_add_session_key(metadata, &config, [1; 32], &policy, Some(100))
            .expect("session call");

        let mut expected = vec![config.pallet_index, config.add_session_key.index];
        expected.push(0x00); // MultiAddress::Id
        expected.extend([1; 32]);
        expected.push(0x01); // Some(duration)
        expected.extend(100u32.to_le_bytes());
        // DeviceFilter::Calls (variant 2) with two pairs, sorted as the
        // runtime's BTreeSet stores them.
        expected.extend([0x02, 2 << 2, 5, 0, 10, 3]);
        assert_eq!(call.hex(), format!("0x{}", hex::encode(&expected)));
    })
}

/// `PassAuthenticate(Option<AuthenticateParams>)` must be SCALE-encoded
/// against the live runtime type, not as an untyped map.
#[test]
#[ignore = "requires network access to a chain with pallet-pass"]
fn pass_authenticate_extension_encodes_against_the_runtime_type() {
    use sube::DynValue;
    futures_lite::future::block_on(async {
        let url = std::env::var("SUBE_E2E_URL").unwrap_or_else(|_| "wss://kreivo.io".into());
        let chain = sube::Sube::connect(&url).await.expect("connect");
        let metadata = chain.metadata();
        let extension = metadata
            .extrinsic
            .extensions
            .iter()
            .find(|extension| extension.identifier == "PassAuthenticate")
            .expect("PassAuthenticate");
        let message = DynValue::obj(&[
            ("context", DynValue::U32(42)),
            ("challenge", DynValue::from([0x11u8; 32])),
            ("authority_id", DynValue::from([0x22u8; 32])),
        ]);
        let credential = DynValue::obj(&[(
            "SubstrateKey",
            DynValue::obj(&[
                ("user_id", DynValue::from([0x44u8; 32])),
                ("message", message),
                (
                    "signature",
                    DynValue::obj(&[("Sr25519", DynValue::from([0x33u8; 64]))]),
                ),
            ]),
        )]);
        // As `PassAuthenticator` builds it: a bare value for the `Option`.
        let value = DynValue::obj(&[
            ("device_id", DynValue::from([0x02u8; 32])),
            ("credential", credential),
        ]);
        let encoded =
            sube::scales::to_vec_with_info(&value, Some((&metadata.registry, extension.ty)))
                .expect("encodes");

        let mut expected = vec![0x01]; // Some
        expected.extend([0x02; 32]);
        expected.push(0x01); // PassCredential::SubstrateKey
        expected.extend([0x44; 32]);
        expected.extend(42u32.to_le_bytes());
        expected.extend([0x11; 32]);
        expected.extend([0x22; 32]);
        expected.push(0x01); // MultiSignature::Sr25519
        expected.extend([0x33; 64]);
        assert_eq!(hex::encode(encoded), hex::encode(expected));
    })
}

/// A substrate-key device registers with substrate-keys'
/// `KeyRegistration { public, message, signature }`, and its device id is the
/// device account.
#[test]
#[ignore = "requires network access to a chain with pallet-pass"]
fn substrate_key_attestation_encodes_against_the_runtime_type() {
    use libwallet::vault::{Simple, Vault};
    use pass::DeviceAuthenticator as _;
    futures_lite::future::block_on(async {
        let url = std::env::var("SUBE_E2E_URL").unwrap_or_else(|_| "wss://kreivo.io".into());
        let chain = sube::Sube::connect(&url).await.expect("connect");
        let metadata = chain.metadata();
        let config = PassRuntimeConfig::discover(metadata).expect("pass config");

        let phrase = "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo \
                      zoo zoo zoo zoo zoo vote";
        let mut vault = libwallet::Substrate::new(Simple::<()>::from_phrase(phrase).unwrap());
        let signer = vault.unlock(None, ()).await.ok().expect("unlock");
        let public = signer.public();
        let device = pass::wallet::WalletDevice::new(
            &signer,
            public.as_ref(),
            pass::wallet::SignatureType::Sr25519,
        );
        let request = pass::workflow::AttestationRequest {
            user_id: pass::HashedUserId([0x11; 32]),
            pass_account: pass::Account([0x55; 32]),
            authority_id: pass::AuthorityId([0x22; 32]),
            context: 42,
            block_hash: [0x66; 32],
            challenge: [0x33; 32],
        };
        let attestation = device.attest(&request).await.expect("attest");
        assert_eq!(attestation.device_id.0[..], public.as_ref()[..]);

        let call = sube::extrinsic::prepare_call(
            metadata,
            &format!("{}/{}", config.pallet, config.register.name),
            &sube::DynValue::obj(&[
                ("user", sube::DynValue::from([0x11u8; 32])),
                ("attestation", attestation.runtime_value()),
            ]),
        )
        .expect("register call");

        let mut expected = vec![config.pallet_index, config.register.index];
        expected.extend([0x11; 32]); // user
        expected.push(0x01); // attestation: SubstrateKey
        expected.extend(public.as_ref()); // public
        expected.extend(42u32.to_le_bytes()); // message.context
        expected.extend([0x33; 32]); // message.challenge
        expected.extend([0x22; 32]); // message.authority_id
        expected.push(0x01); // MultiSignature::Sr25519
        // The sr25519 signature itself is randomized; check the layout.
        assert_eq!(call.bytes.len(), expected.len() + 64);
        assert_eq!(
            hex::encode(&call.bytes[..expected.len()]),
            hex::encode(&expected)
        );
    })
}
