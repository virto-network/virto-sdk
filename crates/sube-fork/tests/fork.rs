//! End-to-end tests: sube talking to an in-process fork through `ChainHead`.
//!
//! They need a full-state snapshot of Kreivo (~25 MB), so they're ignored by
//! default:
//!
//! ```sh
//! cargo run -p sube-fork --example snapshot --release -- wss://kreivo.io kreivo.snap
//! SUBE_FORK_SNAPSHOT=$PWD/kreivo.snap cargo test -p sube-fork --release -- --ignored
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use blake2::{Blake2b, Digest, digest::typenum::U16};
use libwallet::{
    Substrate,
    vault::{Simple, Vault as _, utils::DerivedSigner},
};
use serde_json::{Value, json};
use sube::libwallet::{LibwalletSigner, SignatureScheme};
use sube::rpc::{Rpc as _, RpcSubscription as _};
use sube::{ChainEvent, DispatchOutcome, Response, Text, TransactionOptions, WaitFor};
use sube_fork::{Diff, Fork, ForkRpc, ForkSube, STATE_HISTORY, Snapshot};

const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon \
                      abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon \
                      abandon art";
const UNIT: u128 = 1_000_000_000_000;

type Signer = LibwalletSigner<DerivedSigner>;

fn fork() -> Rc<RefCell<Fork>> {
    let path =
        std::env::var("SUBE_FORK_SNAPSHOT").expect("SUBE_FORK_SNAPSHOT names a snapshot file");
    let snapshot = Snapshot::load(path).expect("loads the snapshot");
    Rc::new(RefCell::new(Fork::new(snapshot).expect("boots the fork")))
}

async fn signer() -> (Signer, [u8; 32]) {
    let mut vault = Substrate::new(Simple::<()>::from_phrase(PHRASE).expect("phrase"));
    let account: DerivedSigner = vault.unlock(None, ()).await.expect("unlock");
    let id: [u8; 32] = account.public().as_ref().try_into().unwrap();
    (
        LibwalletSigner::new(account, id, SignatureScheme::Sr25519),
        id,
    )
}

fn twox128(b: &[u8]) -> [u8; 16] {
    let mut o = [0u8; 16];
    o[..8].copy_from_slice(&twox_hash::XxHash64::oneshot(0, b).to_le_bytes());
    o[8..].copy_from_slice(&twox_hash::XxHash64::oneshot(1, b).to_le_bytes());
    o
}

/// `System::Account(id)`, a `Blake2_128Concat` map.
fn account_key(id: &[u8]) -> Vec<u8> {
    let hashed: [u8; 16] = Blake2b::<U16>::digest(id).into();
    [&twox128(b"System")[..], &twox128(b"Account"), &hashed, id].concat()
}

/// Give `id` an account holding `free`.
fn fund(fork: &Rc<RefCell<Fork>>, id: &[u8], free: u128) {
    // AccountInfo { nonce, consumers, providers, sufficients: u32,
    //               data: { free, reserved, frozen, flags: u128 } }
    let (nonce, consumers, providers, sufficients) = (0u32, 0u32, 1u32, 0u32);
    let new_logic_flags = 1u128 << 127;
    let info = [
        &nonce.to_le_bytes()[..],
        &consumers.to_le_bytes(),
        &providers.to_le_bytes(),
        &sufficients.to_le_bytes(),
        &free.to_le_bytes(),
        &0u128.to_le_bytes(),
        &0u128.to_le_bytes(),
        &new_logic_flags.to_le_bytes(),
    ]
    .concat();
    let mut diff = Diff::default();
    diff.insert(account_key(id), info);
    fork.borrow_mut().set_storage(diff).unwrap();
}

fn free_balance(fork: &Rc<RefCell<Fork>>, id: &[u8]) -> Option<u128> {
    let fork = fork.borrow();
    let info = fork.storage(fork.head_index(), None, &account_key(id))?;
    Some(u128::from_le_bytes(info[16..32].try_into().unwrap()))
}

fn hex0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

async fn transfer_extrinsic(
    sube: &mut ForkSube,
    signer: &Signer,
    dest: [u8; 32],
    value: u128,
    options: TransactionOptions,
) -> sube::EncodedExtrinsic {
    let body = format!("(dest:MultiAddress::Id({});value:{value})", hex0x(&dest));
    let call = sube
        .prepare_call("balances/transfer_keep_alive", &Text(&body))
        .unwrap();
    sube.build_transaction(&call, signer, options)
        .await
        .unwrap()
}

async fn transfer(
    sube: &mut ForkSube,
    signer: &Signer,
    dest: [u8; 32],
    value: u128,
    options: TransactionOptions,
) -> sube::Result<sube::TransactionReceipt> {
    let ext = transfer_extrinsic(sube, signer, dest, value, options).await;
    sube.submit_transaction(&ext, WaitFor::Finalized).await
}

async fn rpc(session: &mut ForkRpc, method: &str, params: Value) -> Value {
    let raw = session
        .rpc(method, &params.to_string())
        .await
        .expect(method);
    serde_json::from_str(&raw).unwrap()
}

async fn next_event(session: &mut ForkRpc) -> Value {
    let (_, event) = session.next_event().await.expect("an event");
    serde_json::from_str(&event).unwrap()
}

#[test]
#[ignore]
fn sube_queries_and_submits() {
    smol::block_on(async {
        let fork = fork();
        let (signer, me) = signer().await;
        fund(&fork, &me, 1_000 * UNIT);
        let mut sube = sube_fork::connect(fork.clone()).await.expect("connects");

        match sube
            .query(&format!("system/account/{}", hex0x(&me)))
            .await
            .expect("query")
        {
            Response::Value(entry, meta) => {
                assert!(entry.to_text(&meta.registry).unwrap().contains("nonce"))
            }
            other => panic!("unexpected {other:?}"),
        }

        // Default options: a mortal era anchored at the fork's head, and the
        // nonce read from the fork's state.
        let head = fork.borrow().head().number;
        let dest = [0x42; 32];
        let receipt = transfer(
            &mut sube,
            &signer,
            dest,
            3 * UNIT,
            TransactionOptions::default(),
        )
        .await
        .expect("transfer");
        assert_eq!(
            receipt.dispatch_outcome,
            DispatchOutcome::Success,
            "{receipt:?}"
        );
        assert_eq!(receipt.extrinsic_index, Some(2), "after the two inherents");
        assert!(
            receipt
                .events
                .iter()
                .any(|e| e.pallet == "Balances" && e.variant == "Transfer"),
            "{receipt:?}"
        );
        assert_eq!(
            fork.borrow().head().number,
            head + 1,
            "one block per submission"
        );
        assert_eq!(free_balance(&fork, &dest), Some(3 * UNIT));

        // The nonce moved on the fork and sube picks it up.
        let receipt = transfer(
            &mut sube,
            &signer,
            dest,
            UNIT,
            TransactionOptions::default(),
        )
        .await
        .expect("second transfer");
        assert_eq!(receipt.dispatch_outcome, DispatchOutcome::Success);
        assert_eq!(free_balance(&fork, &dest), Some(4 * UNIT));
    });
}

#[test]
#[ignore]
fn failed_dispatch_is_included_and_invalid_is_rejected() {
    smol::block_on(async {
        let fork = fork();
        let (signer, me) = signer().await;
        fund(&fork, &me, 10 * UNIT);
        let mut sube = sube_fork::connect(fork.clone()).await.expect("connects");

        // Pays fees but can't transfer more than it has: included, failed.
        let receipt = transfer(
            &mut sube,
            &signer,
            [0x43; 32],
            1_000 * UNIT,
            TransactionOptions::default(),
        )
        .await
        .expect("included");
        assert!(
            matches!(receipt.dispatch_outcome, DispatchOutcome::Failed(_)),
            "{receipt:?}"
        );
        assert!(
            free_balance(&fork, &me).unwrap() < 10 * UNIT,
            "fees were charged"
        );

        // A future nonce passes pool validation but never applies; no block.
        let head = fork.borrow().head().number;
        let err = transfer(
            &mut sube,
            &signer,
            [0x43; 32],
            UNIT,
            TransactionOptions::default().nonce(99),
        )
        .await
        .expect_err("future nonce");
        assert!(err.to_string().contains("Future"), "{err}");
        assert_eq!(fork.borrow().head().number, head);
    });
}

#[test]
#[ignore]
fn time_travel_moves_the_relay_clock() {
    smol::block_on(async {
        let fork = fork();
        let mut sube = sube_fork::connect(fork.clone()).await.expect("connects");
        async fn now(sube: &mut ForkSube) -> u64 {
            match sube.query("timestamp/now").await.expect("query") {
                Response::Value(entry, meta) => entry
                    .to_text(&meta.registry)
                    .unwrap()
                    .trim()
                    .parse()
                    .expect("a number"),
                other => panic!("unexpected {other:?}"),
            }
        }
        fork.borrow_mut().new_block().unwrap();
        let before = now(&mut sube).await;
        let (relay_before, _) = fork.borrow().relay_clock();
        fork.borrow_mut().time_travel(600);
        fork.borrow_mut().new_block().unwrap();
        assert_eq!(now(&mut sube).await - before, 600 * 6_000, "one hour later");
        assert_eq!(fork.borrow().relay_clock().0 - relay_before, 600);
    });
}

#[test]
#[ignore]
fn waiting_session_wakes_when_another_task_builds_a_block() {
    smol::block_on(async {
        let fork = fork();
        let mut sube = sube_fork::connect(fork.clone()).await.expect("connects");
        let builder = fork.clone();
        // The first future parks on an empty queue; the second builds a block
        // on the same thread, which must wake the first.
        let (event, built) = smol::future::zip(sube.next_finalized(), async move {
            smol::future::yield_now().await;
            builder.borrow_mut().new_block().unwrap()
        })
        .await;
        match event.expect("an event") {
            ChainEvent::Finalized { hashes, .. } => assert_eq!(hashes, vec![hex0x(&built.hash)]),
            other => panic!("unexpected {other:?}"),
        }
    });
}

#[test]
#[ignore]
fn map_queries_iterate_storage() {
    smol::block_on(async {
        let mut sube = sube_fork::connect(fork()).await.expect("connects");
        match sube
            .query("communityMemberships/collection")
            .await
            .expect("map query")
        {
            Response::ValueSet(entries, _) => {
                assert!(!entries.is_empty(), "Kreivo has collections")
            }
            other => panic!("unexpected {other:?}"),
        }
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
fn second_session_reads_bodies_and_broadcasts() {
    smol::block_on(async {
        let fork = fork();
        let (signer, me) = signer().await;
        fund(&fork, &me, 100 * UNIT);
        let mut sube = sube_fork::connect(fork.clone()).await.expect("connects");
        let built = fork.borrow_mut().new_block().unwrap();

        let mut session = ForkRpc::new(fork.clone());
        let follow = session
            .subscribe("chainHead_v1_follow", "[false]")
            .await
            .unwrap();
        next_event(&mut session).await;
        rpc(
            &mut session,
            "chainHead_v1_body",
            json!([follow, hex0x(&built.hash)]),
        )
        .await;
        let body = next_event(&mut session).await;
        assert_eq!(body["event"], "operationBodyDone");
        assert_eq!(
            body["value"].as_array().unwrap().len(),
            2,
            "the two inherents"
        );

        // A broadcast from the second session builds a block sube sees.
        let ext = transfer_extrinsic(
            &mut sube,
            &signer,
            [0x44; 32],
            UNIT,
            TransactionOptions::default(),
        )
        .await;
        let id = rpc(
            &mut session,
            "transaction_v1_broadcast",
            json!([hex0x(&ext.bytes)]),
        )
        .await;
        assert!(id.is_string(), "accepted: {id}");
        assert_eq!(free_balance(&fork, &[0x44; 32]), Some(UNIT));
        assert!(matches!(
            sube.next_finalized().await,
            Ok(ChainEvent::Finalized { .. })
        ));
    });
}

#[test]
#[ignore]
fn dev_methods_over_json_rpc() {
    smol::block_on(async {
        let fork = fork();
        let mut session = ForkRpc::new(fork.clone());
        let start = fork.borrow().head().number;

        let methods = rpc(&mut session, "rpc_methods", json!([])).await;
        assert!(
            methods["methods"]
                .as_array()
                .unwrap()
                .contains(&json!("dev_newBlock"))
        );

        let follow = session
            .subscribe("chainHead_v1_follow", "[true]")
            .await
            .unwrap();
        let initialized = next_event(&mut session).await;
        assert_eq!(initialized["event"], "initialized");
        assert!(initialized["finalizedBlockRuntime"]["spec"]["specName"].is_string());

        // dev_setStorage, read back through chainHead_v1_storage.
        rpc(
            &mut session,
            "dev_setStorage",
            json!([[["0xdeadbeef", "0x2a"]]]),
        )
        .await;
        let head = hex0x(&fork.borrow().head().hash);
        let query = json!([follow, head, [{ "key": "0xdeadbeef", "type": "value" }], null]);
        let started = rpc(&mut session, "chainHead_v1_storage", query).await;
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
        let relay = rpc(&mut session, "dev_timeTravel", json!([10])).await;
        assert_eq!(relay, json!(fork.borrow().relay_clock().0 + 10));
        let hash = rpc(&mut session, "dev_newBlock", json!([{ "count": 2 }])).await;
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
        let by_height = rpc(&mut session, "archive_v1_hashByHeight", json!([start + 1])).await;
        assert_eq!(by_height.as_array().unwrap().len(), 1);
        let call = rpc(
            &mut session,
            "archive_v1_call",
            json!([hash, "Core_version", "0x"]),
        )
        .await;
        assert_eq!(call["success"], true);
        let sub = session
            .subscribe(
                "archive_v1_storage",
                &json!([hash, [{ "key": "0xdeadbeef", "type": "hash" }], null]).to_string(),
            )
            .await
            .unwrap();
        let (event_sub, item) = session.next_event().await.unwrap();
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
    smol::block_on(async {
        const BLOCKS: usize = STATE_HISTORY + 44;
        let fork = fork();
        let (_, me) = signer().await;
        fund(&fork, &me, 7 * UNIT);
        let first = fork.borrow().head().number;
        for _ in 0..BLOCKS {
            fork.borrow_mut().new_block().unwrap();
        }
        {
            let f = fork.borrow();
            assert!(f.view(1).is_none(), "old state is pruned");
            assert!(f.view(f.head_index() - STATE_HISTORY).is_some());
            assert_eq!(
                f.index_of_number(first + 1),
                Some(1),
                "old headers still resolve"
            );
            assert!(f.blocks()[1].body.is_none());
        }
        assert_eq!(
            free_balance(&fork, &me),
            Some(7 * UNIT),
            "pruned diffs live on in the base"
        );
        assert!(fork.borrow_mut().call(1, "Core_version", &[]).is_err());
        let mut sube = sube_fork::connect(fork.clone())
            .await
            .expect("connects after pruning");
        sube.query("timestamp/now").await.expect("queries the head");
    });
}
