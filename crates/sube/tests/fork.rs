//! sube against an in-process fork of Kreivo (feature `fork`).
//!
//! These need a full-state snapshot (~25 MB), so they're ignored by default:
//!
//! ```sh
//! cargo run -p sube --example fork_snapshot --features fork,wss --release -- wss://kreivo.io kreivo.snap
//! SUBE_FORK_SNAPSHOT=$PWD/kreivo.snap cargo test -p sube --features fork,libwallet --release --test fork -- --ignored
//! ```

use std::cell::RefCell;
use std::future::IntoFuture;
use std::rc::Rc;

use blake2::{Blake2b, Digest, digest::typenum::U16};
use libwallet::{
    Substrate,
    vault::{Simple, Vault as _, utils::DerivedSigner},
};
use sube::fork::{Diff, Fork, ForkRpc, SharedFork, Snapshot};
use sube::libwallet::{LibwalletSigner, SignatureScheme};
use sube::{
    Backend, ChainEvent, DispatchOutcome, Error, Response, Sube, Text, TransactionOptions, WaitFor,
};

const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon \
                      abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon \
                      abandon art";
const UNIT: u128 = 1_000_000_000_000;

type Signer = LibwalletSigner<DerivedSigner>;

fn snapshot_path() -> String {
    std::env::var("SUBE_FORK_SNAPSHOT").expect("SUBE_FORK_SNAPSHOT names a snapshot file")
}

fn fork() -> SharedFork {
    let snapshot = Snapshot::load(snapshot_path()).expect("loads the snapshot");
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
fn fund(fork: &SharedFork, id: &[u8], free: u128) {
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

fn free_balance(fork: &SharedFork, id: &[u8]) -> Option<u128> {
    let fork = fork.borrow();
    let info = fork.storage(fork.head_index(), None, &account_key(id))?;
    Some(u128::from_le_bytes(info[16..32].try_into().unwrap()))
}

fn hex0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

async fn transfer_extrinsic(
    sube: &mut Sube,
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
    sube: &mut Sube,
    signer: &Signer,
    dest: [u8; 32],
    value: u128,
    options: TransactionOptions,
) -> sube::Result<sube::TransactionReceipt> {
    let ext = transfer_extrinsic(sube, signer, dest, value, options).await;
    sube.submit_transaction(&ext, WaitFor::Finalized).await
}

#[test]
fn a_missing_snapshot_is_a_node_error() {
    let connected = smol::block_on(Sube::connect("fork:/nonexistent/kreivo.snap"));
    assert!(matches!(connected, Err(Error::Node(e)) if e.contains("snapshot")));
}

#[test]
fn one_liner_queries_refuse_forks() {
    let response = smol::block_on(sube::sube("fork:kreivo.snap/system/number").into_future());
    assert!(matches!(response, Err(Error::Node(e)) if e.contains("Sube::connect")));
}

#[test]
#[ignore]
fn connects_by_snapshot_path() {
    smol::block_on(async {
        let mut sube = Sube::connect(&format!("fork:{}", snapshot_path()))
            .await
            .expect("boots a fork");
        let fork = sube.fork().expect("a fork backend").clone();
        let built = fork.borrow_mut().new_block().unwrap();
        match sube.next_finalized().await.expect("an event") {
            ChainEvent::Finalized { hashes, .. } => assert_eq!(hashes, vec![hex0x(&built.hash)]),
            other => panic!("unexpected {other:?}"),
        }

        // Reconnecting opens a new session on the same fork.
        sube.reconnect().await.expect("reconnects");
        assert!(Rc::ptr_eq(&fork, sube.fork().unwrap()));
        sube.query("timestamp/now")
            .await
            .expect("queries after reconnecting");
    });
}

#[test]
#[ignore]
fn waiting_for_a_block_wakes_when_another_task_builds_one() {
    smol::block_on(async {
        let fork = fork();
        let mut sube = Sube::connect_fork(fork.clone()).await.expect("connects");
        // The first future parks on an empty queue; the second builds a block
        // on the same thread, which must wake the first.
        let (event, built) = smol::future::zip(sube.next_finalized(), async move {
            smol::future::yield_now().await;
            fork.borrow_mut().new_block().unwrap()
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
fn the_finalized_head_follows_blocks_built_between_operations() {
    smol::block_on(async {
        let fork = fork();
        let mut sube = Sube::connect_fork(fork.clone()).await.expect("connects");
        fork.borrow_mut().new_block().unwrap();
        let built = fork.borrow_mut().new_block().unwrap();
        // Nothing was asked of the chain since those blocks: the head must
        // not be the one the session started with.
        let head = sube.backend().block_info(None).await.expect("the head");
        assert_eq!(head.hash, built.hash);
        assert_eq!(head.number, u64::from(built.number));
    });
}

#[test]
#[ignore]
fn queries_and_submits() {
    smol::block_on(async {
        let fork = fork();
        let (signer, me) = signer().await;
        fund(&fork, &me, 1_000 * UNIT);
        let mut sube = Sube::connect_fork(fork.clone()).await.expect("connects");

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
        let mut sube = Sube::connect_fork(fork.clone()).await.expect("connects");

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
fn map_queries_iterate_storage() {
    smol::block_on(async {
        let mut sube = Sube::connect_fork(fork()).await.expect("connects");
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
fn broadcast_from_another_session_reaches_sube() {
    smol::block_on(async {
        let fork = fork();
        let (signer, me) = signer().await;
        fund(&fork, &me, 100 * UNIT);
        let mut sube = Sube::connect_fork(fork.clone()).await.expect("connects");

        let ext = transfer_extrinsic(
            &mut sube,
            &signer,
            [0x44; 32],
            UNIT,
            TransactionOptions::default(),
        )
        .await;
        let mut session = ForkRpc::new(fork.clone());
        let params = format!(r#"["{}"]"#, hex0x(&ext.bytes));
        let id = session
            .request("transaction_v1_broadcast", &params)
            .unwrap();
        assert!(id.starts_with('"'), "accepted: {id}");
        assert_eq!(free_balance(&fork, &[0x44; 32]), Some(UNIT));
        assert!(matches!(
            sube.next_finalized().await,
            Ok(ChainEvent::Finalized { .. })
        ));
    });
}
