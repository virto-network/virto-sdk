//! Read-only live check of the session call encoding against a current
//! pallet-pass runtime (Kreivo by default; override with `SUBE_E2E_URL`).
//!
//! cargo test -p pass --test live_session_call -- --ignored

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
        assert_eq!(call.hex, format!("0x{}", hex::encode(&expected)));
    })
}
