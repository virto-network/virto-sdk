//! End-to-end tests of the fork and its JSON-RPC session on a real chain.
//! Tests that sign transactions through sube live in sube (feature `fork`).
//!
//! They need a full-state snapshot of Kreivo (~25 MB), so they're ignored by
//! default. Download one with sube's `fork_snapshot` example, then:
//!
//! ```sh
//! SUBE_FORK_SNAPSHOT=$PWD/kreivo.snap cargo test -p sube-fork --release -- --ignored
//! ```

use std::cell::RefCell;

use codec::Encode;
use serde_json::{Value, json};
use sube_fork::{Diff, Fork, ForkRpc, STATE_HISTORY, SharedFork, Snapshot};

fn fork() -> SharedFork {
    let path =
        std::env::var("SUBE_FORK_SNAPSHOT").expect("SUBE_FORK_SNAPSHOT names a snapshot file");
    let snapshot = Snapshot::load(path).expect("loads the snapshot");
    SharedFork::new(RefCell::new(Fork::new(snapshot).expect("boots the fork")))
}

fn hex0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

fn twox128(b: &[u8]) -> [u8; 16] {
    let mut o = [0u8; 16];
    o[..8].copy_from_slice(&twox_hash::XxHash64::oneshot(0, b).to_le_bytes());
    o[8..].copy_from_slice(&twox_hash::XxHash64::oneshot(1, b).to_le_bytes());
    o
}

fn timestamp(fork: &Fork) -> u64 {
    let key = [twox128(b"Timestamp"), twox128(b"Now")].concat();
    let now = fork
        .storage(fork.head_index(), None, &key)
        .expect("Timestamp::Now");
    u64::from_le_bytes(now[..8].try_into().unwrap())
}

fn rpc(session: &mut ForkRpc, method: &str, params: Value) -> Value {
    let raw = session.request(method, &params.to_string()).expect(method);
    serde_json::from_str(&raw).unwrap()
}

async fn next_event(session: &mut ForkRpc) -> Value {
    let (_, event) = session.next_event().await;
    serde_json::from_str(&event).unwrap()
}

#[test]
#[ignore]
fn time_travel_moves_the_relay_clock() {
    let fork = fork();
    let mut f = fork.borrow_mut();
    f.new_block().unwrap();
    let (before, relay_before) = (timestamp(&f), f.relay_clock().0);
    f.time_travel(600);
    f.new_block().unwrap();
    assert_eq!(timestamp(&f) - before, 600 * 6_000, "one hour later");
    assert_eq!(f.relay_clock().0 - relay_before, 600);
}

#[test]
#[ignore]
fn waiting_session_wakes_when_another_task_builds_a_block() {
    smol::block_on(async {
        let fork = fork();
        let mut session = ForkRpc::new(fork.clone());
        session.subscribe("chainHead_v1_follow", "[false]").unwrap();
        assert_eq!(next_event(&mut session).await["event"], "initialized");
        // The first future parks on an empty queue; the second builds a block
        // on the same thread, which must wake the first.
        let builder = fork.clone();
        let (event, built) = smol::future::zip(next_event(&mut session), async move {
            smol::future::yield_now().await;
            builder.borrow_mut().new_block().unwrap()
        })
        .await;
        assert_eq!(event["event"], "newBlock");
        assert_eq!(event["blockHash"], hex0x(&built.hash));
    });
}

#[test]
#[ignore]
fn writing_code_switches_the_runtime() {
    let fork = fork();
    let mut f = fork.borrow_mut();
    f.new_block().unwrap();
    let head = f.head_index();
    let code = f.storage(head, None, b":code").unwrap();
    let mut diff = Diff::default();
    diff.insert(&b":code"[..], code);
    f.set_storage(diff).unwrap();

    // Calls at the head use the new runtime; older blocks keep the old one.
    let now = f.call(head, "Core_version", &[]).expect("new runtime");
    let before = f
        .call(head - 1, "Core_version", &[])
        .expect("previous runtime");
    assert_eq!(now, before);
    let next = f.new_block().expect("builds after the switch");
    let block = &f.blocks()[f.index_of(&next.hash).unwrap()];
    assert!(
        block.new_runtime.is_some(),
        "the switch is announced with the next block"
    );
}

#[test]
#[ignore]
fn block_bodies_over_json_rpc() {
    smol::block_on(async {
        let fork = fork();
        let built = fork.borrow_mut().new_block().unwrap();
        let mut session = ForkRpc::new(fork.clone());
        let follow = session.subscribe("chainHead_v1_follow", "[false]").unwrap();
        next_event(&mut session).await;
        rpc(
            &mut session,
            "chainHead_v1_body",
            json!([follow, hex0x(&built.hash)]),
        );
        let body = next_event(&mut session).await;
        assert_eq!(body["event"], "operationBodyDone");
        assert_eq!(
            body["value"].as_array().unwrap().len(),
            2,
            "the two inherents"
        );
    });
}

#[test]
#[ignore]
fn dev_methods_over_json_rpc() {
    smol::block_on(async {
        let fork = fork();
        let mut session = ForkRpc::new(fork.clone());
        let start = fork.borrow().head().number;

        let methods = rpc(&mut session, "rpc_methods", json!([]));
        assert!(
            methods["methods"]
                .as_array()
                .unwrap()
                .contains(&json!("dev_newBlock"))
        );

        let follow = session.subscribe("chainHead_v1_follow", "[true]").unwrap();
        let initialized = next_event(&mut session).await;
        assert_eq!(initialized["event"], "initialized");
        assert!(initialized["finalizedBlockRuntime"]["spec"]["specName"].is_string());

        // dev_setStorage, read back through chainHead_v1_storage.
        rpc(
            &mut session,
            "dev_setStorage",
            json!([[["0xdeadbeef", "0x2a"]]]),
        );
        let head = hex0x(&fork.borrow().head().hash);
        let query = json!([follow, head, [{ "key": "0xdeadbeef", "type": "value" }], null]);
        let started = rpc(&mut session, "chainHead_v1_storage", query);
        assert_eq!(started["result"], "started");
        let items = next_event(&mut session).await;
        assert_eq!(
            items["items"],
            json!([{ "key": "0xdeadbeef", "value": "0x2a" }])
        );
        assert_eq!(
            next_event(&mut session).await["event"],
            "operationStorageDone"
        );

        // dev_timeTravel and dev_newBlock: three follow events per block.
        let relay = rpc(&mut session, "dev_timeTravel", json!([10]));
        assert_eq!(relay, json!(fork.borrow().relay_clock().0 + 10));
        let hash = rpc(&mut session, "dev_newBlock", json!([{ "count": 2 }]));
        assert_eq!(fork.borrow().head().number, start + 2);
        let mut events = Vec::new();
        while let Some((_, e)) = session.try_next_event() {
            events.push(serde_json::from_str::<Value>(&e).unwrap());
        }
        let kinds: Vec<_> = events
            .iter()
            .map(|e| e["event"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            ["newBlock", "bestBlockChanged", "finalized"].repeat(2)
        );
        assert_eq!(events[0]["newRuntime"], Value::Null);
        assert_eq!(events[5]["finalizedBlockHashes"][0], hash);

        // archive_v1: blocks stay addressable by height and callable.
        let by_height = rpc(&mut session, "archive_v1_hashByHeight", json!([start + 1]));
        assert_eq!(by_height.as_array().unwrap().len(), 1);
        let call = rpc(
            &mut session,
            "archive_v1_call",
            json!([hash, "Core_version", "0x"]),
        );
        assert_eq!(call["success"], true);
        let params = json!([hash, [{ "key": "0xdeadbeef", "type": "hash" }], null]);
        let sub = session
            .subscribe("archive_v1_storage", &params.to_string())
            .unwrap();
        let (event_sub, item) = session.next_event().await;
        let item: Value = serde_json::from_str(&item).unwrap();
        assert_eq!(event_sub, sub);
        assert_eq!(
            (item["event"].as_str(), item["key"].as_str()),
            (Some("storage"), Some("0xdeadbeef"))
        );
        assert!(
            item["hash"].is_string(),
            "the value set before both blocks: {item}"
        );
    });
}

#[test]
#[ignore]
fn long_chains_prune_old_state() {
    let fork = fork();
    let mut f = fork.borrow_mut();
    let first = f.head().number;
    let mut diff = Diff::default();
    diff.insert(&b"written before pruning"[..], &b"kept"[..]);
    f.set_storage(diff).unwrap();
    for _ in 0..STATE_HISTORY + 44 {
        f.new_block().unwrap();
    }
    assert!(f.view(1).is_none(), "old state is pruned");
    assert!(f.view(f.head_index() - STATE_HISTORY).is_some());
    assert_eq!(
        f.index_of_number(first + 1),
        Some(1),
        "old headers still resolve"
    );
    assert!(f.blocks()[1].body.is_none());
    assert!(f.call(1, "Core_version", &[]).is_err());
    let kept = f.storage(f.head_index(), None, b"written before pruning");
    assert_eq!(
        kept.as_deref(),
        Some(&b"kept"[..]),
        "pruned diffs live on in the base"
    );
}

/// The smallest module pallet-contracts accepts: it imports its memory and
/// exports `call` and `deploy`.
const EMPTY_CONTRACT: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic, version
    0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // one type: () -> ()
    0x02, 0x10, 0x01, 0x03, b'e', b'n', b'v', 0x06, b'm', b'e', b'm', b'o', b'r', b'y', 0x02, 0x01,
    0x01, 0x01, // import env.memory, one page
    0x03, 0x03, 0x02, 0x00, 0x00, // two functions
    0x07, 0x11, 0x02, 0x04, b'c', b'a', b'l', b'l', 0x00, 0x00, 0x06, b'd', b'e', b'p', b'l', b'o',
    b'y', 0x00, 0x01, // export them
    0x0a, 0x07, 0x02, 0x02, 0x00, 0x0b, 0x02, 0x00, 0x0b, // two empty bodies
];

/// `System::Account(id)` holding `free`.
fn account(id: &[u8; 32], free: u128) -> (Vec<u8>, Vec<u8>) {
    use blake2::{Blake2b, Digest, digest::typenum::U16};
    let hashed: [u8; 16] = Blake2b::<U16>::digest(id).into();
    let key = [&twox128(b"System")[..], &twox128(b"Account"), &hashed, id].concat();
    let (nonce, consumers, providers, sufficients) = (0u32, 0u32, 1u32, 0u32);
    let flags = 1u128 << 127;
    let info = (
        nonce,
        consumers,
        providers,
        sufficients,
        free,
        0u128,
        0u128,
        flags,
    )
        .encode();
    (key, info)
}

#[test]
#[ignore]
fn runtime_calls_can_keep_their_changes() {
    let fork = fork();
    let mut f = fork.borrow_mut();
    let uploader = [7u8; 32];
    let (key, info) = account(&uploader, 1_000_000_000_000_000);
    let mut diff = Diff::default();
    diff.insert(key, info);
    f.set_storage(diff).unwrap();

    // Kreivo only lets communities upload code by extrinsic; its runtime API
    // takes any account. `ContractsApi::upload_code(origin, code,
    // storage_deposit_limit, determinism)` answers `Ok((code_hash, deposit))`.
    let param = (uploader, EMPTY_CONTRACT.to_vec(), None::<u128>, 0u8).encode();
    let code_info = |f: &Fork, code_hash: &[u8]| {
        let key = [
            &twox128(b"Contracts")[..],
            &twox128(b"CodeInfoOf"),
            code_hash,
        ]
        .concat();
        f.storage(f.head_index(), None, &key)
    };

    let head = f.head_index();
    let dry = f.call(head, "ContractsApi_upload_code", &param).unwrap();
    assert_eq!(dry[0], 0, "upload failed: {}", hex0x(&dry));
    let code_hash = dry[1..33].to_vec();
    assert_eq!(code_info(&f, &code_hash), None, "`call` changes nothing");

    let kept = f.call_mut("ContractsApi_upload_code", &param).unwrap();
    assert_eq!(kept, dry);
    assert!(
        code_info(&f, &code_hash).is_some(),
        "`call_mut` keeps the code"
    );
    assert_eq!(f.head_index(), head, "without building a block");

    f.new_block().unwrap();
    assert!(
        code_info(&f, &code_hash).is_some(),
        "and blocks build on it"
    );
}

#[test]
#[ignore]
fn a_fork_resumes_from_its_own_snapshot() {
    let fork = fork();
    let mut f = fork.borrow_mut();
    let (key, info) = account(&[9u8; 32], 42);
    let mut diff = Diff::default();
    diff.insert(key.clone(), info.clone());
    f.set_storage(diff).unwrap();
    f.new_block().unwrap();
    f.time_travel(600);
    f.new_block().unwrap();

    // Through the file format, as a saved fork would be.
    let saved = f.snapshot().encode();
    let mut resumed = Fork::new(Snapshot::decode(&saved).unwrap()).expect("boots its own snapshot");
    assert_eq!(resumed.head().hash, f.head().hash);
    assert_eq!(resumed.head().number, f.head().number);
    assert_eq!(resumed.genesis_hash(), f.genesis_hash());
    assert_eq!(resumed.relay_clock(), f.relay_clock());
    assert_eq!(timestamp(&resumed), timestamp(&f));
    assert_eq!(
        resumed.storage(resumed.head_index(), None, &key),
        Some(info)
    );

    // Both keep building the same chain.
    let (ours, theirs) = (f.new_block().unwrap(), resumed.new_block().unwrap());
    assert_eq!(theirs.number, ours.number);
    assert_eq!(theirs.hash, ours.hash);
    assert_eq!(timestamp(&resumed), timestamp(&f));
}
