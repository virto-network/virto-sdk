# sube-fork

An in-process fork of a live Substrate parachain for local development, in the
spirit of [chopsticks](https://github.com/AcalaNetwork/chopsticks) but as a
[sube](../sube) backend. It was built for Kreivo, which has no testnet. Front-end and
back-end developers get the chain's real logic (common pallets, contracts) without
running zombienet, collators or a relay chain.

- **Instant boot.** A fork starts from a full-state snapshot (Kreivo: ~25 MB,
  168k keys) in about 100 ms, offline. The runtime runs on smoldot's executor
  with wasmi and lazy validation (~40 ms to instantiate; wasmtime takes ~24 s).
- **Real blocks.** Each submitted transaction builds and finalizes a block
  with the real runtime, in ~7–15 ms. State stays readable for the last 256
  blocks (older diffs fold into the base), so block time and memory stay flat:
  ~8 ms per block and ~90 MB after 2,000 blocks. Failed dispatches are included (fees and
  `ExtrinsicFailed`, as on chain); invalid transactions are rejected.
- **Mocked relay chain.** Every block rewrites the fork block's real
  `set_validation_data` inherent: it advances the relay slot and number, marks the
  parent as included and drops inbound XCM. See [`src/para.rs`](src/para.rs).
- **Time travel.** Kreivo's pallets use the relay chain block number as their
  clock, so jumping the relay chain forward moves scheduler, referenda, vesting and
  so on.
- **Plain sube API.** `ForkRpc` implements sube's `Rpc`/`RpcSubscription`, so
  `ChainHead` and `Sube` work unchanged. The chain is shared via
  `Rc<RefCell<Fork>>`, so dev operations stay available while sube holds the
  session. Single-threaded and poll-driven: nothing is spawned.

## Usage

```sh
# Once (or whenever you want fresher state): download a verified snapshot.
cargo run -p sube-fork --example snapshot --release -- wss://kreivo.io kreivo.snap
```

```rust,ignore
use std::{cell::RefCell, rc::Rc};
use sube_fork::{Diff, Fork, Snapshot};

let fork = Rc::new(RefCell::new(Fork::new(Snapshot::load("kreivo.snap")?)?));
let mut sube = sube_fork::connect(fork.clone()).await?;

// Query and submit as against any node; each submission builds a block.
sube.query("timestamp/now").await?;

// Dev operations on the shared handle.
let mut diff = Diff::default();
diff.insert(account_key, account_info); // e.g. fund an account
fork.borrow_mut().set_storage(diff)?;
fork.borrow_mut().time_travel(600); // the next block lands an hour later
fork.borrow_mut().new_block()?;
fork.borrow_mut().policy.magic = true; // accept 0xdeadbeef…cd signatures
```

## JSON-RPC surface

| Methods | Notes |
|---|---|
| `chainHead_v1_*` (`follow`, `header`, `body`, `call`, `storage`, `continue`, `stopOperation`, `unpin`, `unfollow`) | Every block is final at once. `closestDescendantMerkleValue` storage queries return nothing. |
| `transactionWatch_v1_*`, `transaction_v1_broadcast`/`stop` | Instant seal by default (`ForkRpc::instant`). |
| `archive_v1_*` (`genesisHash`, `finalizedHeight`, `hashByHeight`, `header`, `call`, `storage`) | Blocks keep their state for 256 blocks; older headers still resolve. |
| `chainSpec_v1_*`, `system_chain`/`name`/`version`/`properties`, `rpc_methods` | |
| `dev_newBlock [{count}]` | Build blocks, including empty ones. |
| `dev_setStorage [[[key, value\|null], …]]` or `[{key: value}]` | Writes at the head without a block. |
| `dev_timeTravel [relayBlocks]` | The next block lands that many relay blocks (6 s each) after its parent. Returns its relay block number. |

## Tests

Unit tests run anywhere. The end-to-end tests drive sube against a real Kreivo
fork, so they need a snapshot and are ignored by default:

```sh
SUBE_FORK_SNAPSHOT=$PWD/kreivo.snap cargo test -p sube-fork --release -- --ignored
```

## Limitations

- **State roots aren't authoritative.** Like chopsticks, only each call's diff is
  hashed, so block hashes differ from what a real node would produce, and light
  clients and storage proofs can't verify against a fork. A full rebuild of
  Kreivo's trie takes ~200 ms, so real roots are possible later.
- **Parachain assumptions** (cumulus inherent layout, 6 s slots, Aura) live in
  `para.rs`. A dry-run block at boot fails fast if a runtime breaks them.
- **Runtime upgrades** through the chain's own flow (`authorize_upgrade` /
  `apply_authorized_upgrade` via governance) probably never enact, because the
  relay chain's upgrade go-ahead signal isn't mocked. Writing `:code` with
  `set_storage` does switch runtimes.
- **Not yet supported:** outbound XCM inspection and inbound message
  injection; reverting to an earlier block; more than one block per relay slot.
- **Not yet exercised by tests:** Pass/WebAuthn-authenticated extrinsics,
  a real ink! message (contract dry-runs and storage reads are covered), and
  `transaction_v1_broadcast` failure reporting.

## License

GPL-3.0-or-later (see [LICENSE](LICENSE)). The crate builds on smoldot, which is
GPL-3.0-or-later WITH Classpath-exception-2.0.
