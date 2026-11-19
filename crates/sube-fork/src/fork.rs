//! The forked chain: state, local blocks, the transaction pool and the block
//! builder. It knows nothing about JSON-RPC; [`crate::ForkRpc`] sessions serve
//! it to clients.

use std::task::Waker;

use codec::Decode;

use crate::Hash;
use crate::error::{Error, Result};
use crate::executor::{Runtime, RuntimeInfo, SignaturePolicy};
use crate::header::Header;
use crate::para::{self, Relay};
use crate::snapshot::Snapshot;
use crate::storage::{Base, Diff, View};
use crate::util::{blake2_256, hex0x, storage_key};

/// `TransactionSource::External`: a transaction submitted by a user.
const SOURCE_EXTERNAL: u8 = 2;

/// Blocks whose state stays readable, like a pruned node. Older diffs are
/// folded into the base so that reads and memory don't grow with the chain.
pub const STATE_HISTORY: usize = 256;

pub struct Block {
    pub hash: Hash,
    pub parent: Hash,
    pub number: u32,
    /// The SCALE-encoded header.
    pub header: Vec<u8>,
    /// Encoded extrinsics, inherents first; `None` once pruned.
    pub body: Option<Vec<Vec<u8>>>,
    /// The runtime this block switched to, if it changed `:code`.
    pub new_runtime: Option<RuntimeInfo>,
    diff: Diff,
    /// Index of the block whose state holds the `:code` of calls made on top
    /// of this one.
    code_at: usize,
}

/// An extrinsic dropped while building a block.
#[derive(Clone, Debug)]
pub struct Rejected {
    pub hash: Hash,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct Built {
    pub hash: Hash,
    pub number: u32,
    /// The user extrinsics included, with their index in the body.
    pub included: Vec<(Hash, u32)>,
    pub rejected: Vec<Rejected>,
}

/// State and blocks, with the fork block at index 0.
struct Chain {
    base: Base,
    blocks: Vec<Block>,
    /// Blocks up to this index have their diffs in `base`.
    pruned: usize,
}

impl Chain {
    fn head_index(&self) -> usize {
        self.blocks.len() - 1
    }

    fn view(&self, index: usize) -> Option<View<'_>> {
        let layers = self.blocks.get(self.pruned + 1..=index)?;
        (index >= self.pruned).then(|| View::new(&self.base, layers.iter().map(|b| &b.diff)))
    }

    fn head_view(&self) -> View<'_> {
        self.view(self.head_index())
            .expect("the head is never pruned")
    }

    fn prune(&mut self) {
        while self.head_index() - self.pruned > STATE_HISTORY {
            self.pruned += 1;
            let block = &mut self.blocks[self.pruned];
            self.base.apply(core::mem::take(&mut block.diff));
            block.body = None;
        }
    }
}

/// A block built on the head but not committed yet.
struct Candidate {
    block: Block,
    relay: Relay,
    /// The new runtime, if the block changed `:code`.
    runtime: Option<Runtime>,
    included: Vec<(Hash, u32)>,
    rejected: Vec<Rejected>,
}

pub struct Fork {
    chain: Chain,
    runtime: Runtime,
    /// Index of the block whose `:code` `runtime` was compiled from.
    runtime_at: usize,
    /// A runtime set with [`Fork::set_storage`], announced with the next block.
    unannounced_runtime: Option<RuntimeInfo>,
    relay: Relay,
    pub policy: SignaturePolicy,
    pool: Vec<Vec<u8>>,
    /// Everything dropped while building blocks, in order. Sessions keep a
    /// cursor into it to tell watchers.
    rejected: Vec<Rejected>,
    /// Relay blocks between the head and the next block, when time travel
    /// asked for more than one.
    jump: u32,
    genesis_hash: Hash,
    chain_name: String,
    properties: String,
    wakers: Vec<Waker>,
}

impl Fork {
    /// Boot a fork from a snapshot. A block is built and discarded to check
    /// the assumptions of the relay chain mock hold for this runtime.
    pub fn new(snapshot: Snapshot) -> Result<Self> {
        let header = Header::decode(&mut &snapshot.header[..])
            .map_err(|e| Error::Snapshot(format!("fork block header: {e}")))?;
        let base = snapshot.state;
        let runtime = Runtime::from_view(&View::new(&base, []))?;
        let para_id = base
            .main
            .get(&storage_key("ParachainInfo", "ParachainId"))
            .and_then(|v| u32::decode(&mut &v[..]).ok())
            .ok_or_else(|| {
                Error::Layout("not a parachain: no ParachainInfo::ParachainId".into())
            })?;
        let relay = Relay::from_block(&snapshot.extrinsics, para_id)?;
        let fork_block = Block {
            hash: snapshot.block_hash,
            parent: header.parent,
            number: header.number,
            header: snapshot.header,
            body: Some(snapshot.extrinsics),
            new_runtime: None,
            diff: Diff::default(),
            code_at: 0,
        };
        let mut fork = Fork {
            chain: Chain {
                base,
                blocks: vec![fork_block],
                pruned: 0,
            },
            runtime,
            runtime_at: 0,
            unannounced_runtime: None,
            policy: SignaturePolicy {
                trusted_keys: relay.babe_authorities(),
                magic: false,
            },
            relay,
            pool: Vec::new(),
            rejected: Vec::new(),
            jump: 0,
            genesis_hash: snapshot.genesis_hash,
            chain_name: snapshot.chain_name,
            properties: snapshot.properties,
            wakers: Vec::new(),
        };
        fork.execute().map_err(|e| {
            Error::Layout(format!(
                "a test block failed, the relay chain mock doesn't fit this runtime: {e}"
            ))
        })?;
        log::info!(
            "forked {} #{} ({}), {} {}",
            fork.chain_name,
            header.number,
            hex0x(&fork.head().hash),
            fork.runtime.info.spec_name,
            fork.runtime.info.spec_version
        );
        Ok(fork)
    }

    pub fn genesis_hash(&self) -> Hash {
        self.genesis_hash
    }

    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }

    /// Raw JSON of the chain's `system_properties`.
    pub fn properties(&self) -> &str {
        &self.properties
    }

    /// The runtime of the head.
    pub fn runtime(&self) -> &RuntimeInfo {
        &self.runtime.info
    }

    /// All blocks, the fork block first. Every block is final.
    pub fn blocks(&self) -> &[Block] {
        &self.chain.blocks
    }

    pub fn head(&self) -> &Block {
        &self.chain.blocks[self.chain.head_index()]
    }

    pub fn head_index(&self) -> usize {
        self.chain.head_index()
    }

    pub fn index_of(&self, hash: &Hash) -> Option<usize> {
        self.blocks().iter().rposition(|b| &b.hash == hash)
    }

    pub fn index_of_number(&self, number: u32) -> Option<usize> {
        let i = number.checked_sub(self.blocks()[0].number)? as usize;
        (i < self.blocks().len()).then_some(i)
    }

    /// The state at block `index`, unless it was pruned (see [`STATE_HISTORY`]).
    pub fn view(&self, index: usize) -> Option<View<'_>> {
        self.chain.view(index)
    }

    /// A storage value at block `index`; `None` if absent or pruned.
    pub fn storage(&self, index: usize, child: Option<&[u8]>, key: &[u8]) -> Option<Vec<u8>> {
        self.view(index)?.get(child, key).map(<[u8]>::to_vec)
    }

    /// Relay chain block number and slot of the head.
    pub fn relay_clock(&self) -> (u32, u64) {
        (self.relay.number, self.relay.slot)
    }

    /// Relay chain block number of the next block.
    pub fn next_relay_number(&self) -> u32 {
        self.relay.number + self.jump.max(1)
    }

    /// Run a runtime API at block `index` without changing state.
    pub fn call(&mut self, index: usize, function: &str, param: &[u8]) -> Result<Vec<u8>> {
        let state = self.chain.view(index).ok_or(Error::UnknownBlock)?;
        let output = if self.chain.blocks[index].code_at == self.runtime_at {
            self.runtime.call(&state, &self.policy, function, param)?
        } else {
            // Before a local runtime upgrade: compile that runtime again.
            Runtime::from_view(&state)?.call(&state, &self.policy, function, param)?
        };
        Ok(output.output)
    }

    /// Validate an extrinsic against the head and queue it for the next block.
    /// Returns its hash.
    pub fn submit(&mut self, ext: Vec<u8>) -> Result<Hash> {
        let head = self.head();
        let param = [&[SOURCE_EXTERNAL][..], &ext, &head.hash].concat();
        let out = self.call(
            self.head_index(),
            "TaggedTransactionQueue_validate_transaction",
            &param,
        )?;
        match out.split_first() {
            Some((0, _)) => {}
            Some((1, error)) => return Err(Error::Invalid(describe_validity_error(error))),
            _ => {
                return Err(Error::Runtime(
                    "malformed validate_transaction result".into(),
                ));
            }
        }
        let hash = blake2_256(&ext);
        self.pool.push(ext);
        Ok(hash)
    }

    pub fn rejected(&self) -> &[Rejected] {
        &self.rejected
    }

    /// Make the next block land `relay_blocks` relay blocks (6s each) after its
    /// parent instead of one, e.g. `600` for an hour later. Calls add up.
    /// Runtimes that use the relay chain as their clock see time pass.
    pub fn time_travel(&mut self, relay_blocks: u32) {
        self.jump = self.jump.saturating_add(relay_blocks);
    }

    /// Build a block with the pooled extrinsics, even if it ends up empty.
    pub fn new_block(&mut self) -> Result<Built> {
        let candidate = self.execute()?;
        Ok(self.commit(candidate))
    }

    /// Build a block only if a pooled extrinsic makes it in, as an
    /// instant-seal node does on submission. Extrinsics the runtime refuses
    /// (a future nonce passes pool validation, for example) are dropped and
    /// recorded in [`Fork::rejected`] either way.
    pub fn include_pool(&mut self) -> Result<Option<Built>> {
        if self.pool.is_empty() {
            return Ok(None);
        }
        let candidate = self.execute()?;
        if !candidate.included.is_empty() {
            return Ok(Some(self.commit(candidate)));
        }
        self.pool.clear();
        self.rejected.extend(candidate.rejected);
        self.wake();
        Ok(None)
    }

    /// Write storage at the head without building a block, e.g. to fund an
    /// account. Writing `:code` switches the runtime, which is announced with
    /// the next block.
    pub fn set_storage(&mut self, diff: Diff) -> Result<()> {
        let head = self.chain.head_index();
        let runtime = match diff.touches(b":code") {
            true => Some(Runtime::from_view(&self.chain.head_view().with(&diff))?),
            false => None,
        };
        if head == self.chain.pruned {
            self.chain.base.apply(diff);
        } else {
            self.chain.blocks[head].diff.merge(diff);
        }
        if let Some(runtime) = runtime {
            self.unannounced_runtime = Some(runtime.info.clone());
            self.runtime = runtime;
            self.runtime_at = head;
            self.chain.blocks[head].code_at = head;
        }
        Ok(())
    }

    /// Have `waker` woken when the next block is built.
    pub fn register_waker(&mut self, waker: &Waker) {
        if !self.wakers.iter().any(|w| w.will_wake(waker)) {
            self.wakers.push(waker.clone());
        }
    }

    fn wake(&mut self) {
        self.wakers.drain(..).for_each(Waker::wake);
    }

    /// Build a block with the pool on top of the head. Only the VM is touched.
    fn execute(&mut self) -> Result<Candidate> {
        let parent = self.head();
        let (parent_hash, number) = (parent.hash, parent.number + 1);
        let mut relay = self.relay.clone();
        let inherents = relay.advance(&parent.header, self.jump.max(1));

        let head = self.chain.head_view();
        let (runtime, policy) = (&mut self.runtime, &self.policy);
        let mut pending = Diff::default();
        let mut run = |pending: &Diff, function: &str, param: &[u8]| {
            runtime.call(&head.with(pending), policy, function, param)
        };

        let header = Header::pre_runtime(parent_hash, number, para::AURA_ENGINE, inherents.slot);
        pending.merge(run(&pending, "Core_initialize_block", &header.encode())?.changes);

        let mut body = Vec::new();
        for (name, ext) in [
            ("set_validation_data", inherents.validation_data),
            ("timestamp", inherents.timestamp),
        ] {
            let out = run(&pending, "BlockBuilder_apply_extrinsic", &ext)?;
            if out.output != [0, 0] {
                return Err(Error::Layout(format!(
                    "inherent {name} failed: {}",
                    describe_apply_result(&out.output)
                )));
            }
            pending.merge(out.changes);
            body.push(ext);
        }

        let (mut included, mut rejected) = (Vec::new(), Vec::new());
        for ext in &self.pool {
            let hash = blake2_256(ext);
            match run(&pending, "BlockBuilder_apply_extrinsic", ext) {
                // Ok(_) is included whether the dispatch succeeded or failed:
                // a failed dispatch still pays fees and emits ExtrinsicFailed.
                Ok(out) if out.output.first() == Some(&0) => {
                    pending.merge(out.changes);
                    included.push((hash, body.len() as u32));
                    body.push(ext.clone());
                }
                Ok(out) => rejected.push(Rejected {
                    hash,
                    reason: describe_apply_result(&out.output),
                }),
                Err(trap) => rejected.push(Rejected {
                    hash,
                    reason: trap.to_string(),
                }),
            }
        }

        let finalized = run(&pending, "BlockBuilder_finalize_block", &[])?;
        pending.merge(finalized.changes);
        let header = finalized.output;
        let runtime = match pending.touches(b":code") {
            true => Some(Runtime::from_view(&head.with(&pending))?),
            false => None,
        };
        let block = Block {
            hash: blake2_256(&header),
            parent: parent_hash,
            number,
            header,
            body: Some(body),
            new_runtime: None,
            diff: pending,
            code_at: self.runtime_at,
        };
        Ok(Candidate {
            block,
            relay,
            runtime,
            included,
            rejected,
        })
    }

    /// Append a built block. The pool was all included or rejected.
    fn commit(&mut self, candidate: Candidate) -> Built {
        let Candidate {
            mut block,
            relay,
            runtime,
            included,
            rejected,
        } = candidate;
        let index = self.chain.blocks.len();
        if let Some(runtime) = runtime {
            block.new_runtime = Some(runtime.info.clone());
            block.code_at = index;
            self.runtime = runtime;
            self.runtime_at = index;
        } else {
            block.new_runtime = self.unannounced_runtime.take();
        }
        let built = Built {
            hash: block.hash,
            number: block.number,
            included,
            rejected: rejected.clone(),
        };
        log::debug!("built #{} {}", block.number, hex0x(&block.hash));
        self.relay = relay;
        self.pool.clear();
        self.jump = 0;
        self.rejected.extend(rejected);
        self.chain.blocks.push(block);
        self.chain.prune();
        self.wake();
        built
    }
}

/// Human readable `ApplyExtrinsicResult` failure.
fn describe_apply_result(out: &[u8]) -> String {
    match out {
        [0, 0, ..] => "ok".into(),
        [0, 1, rest @ ..] => format!("dispatch error 0x{}", hex::encode(rest)),
        [1, rest @ ..] => describe_validity_error(rest),
        _ => format!("malformed apply result 0x{}", hex::encode(out)),
    }
}

/// Human readable `TransactionValidityError`.
fn describe_validity_error(e: &[u8]) -> String {
    const INVALID: &[&str] = &[
        "Call",
        "Payment",
        "Future",
        "Stale",
        "BadProof",
        "AncientBirthBlock",
        "ExhaustsResources",
        "Custom",
        "BadMandatory",
        "MandatoryValidation",
        "BadSigner",
        "IndeterminateImplicit",
        "UnknownOrigin",
    ];
    const UNKNOWN: &[&str] = &["CannotLookup", "NoUnsignedValidator", "Custom"];
    let (kind, names) = match e.first() {
        Some(0) => ("Invalid", INVALID),
        Some(1) => ("Unknown", UNKNOWN),
        _ => return format!("validity error 0x{}", hex::encode(e)),
    };
    match (e.get(1).and_then(|&i| names.get(usize::from(i))), e.get(2)) {
        (Some(&"Custom"), Some(code)) => format!("{kind}::Custom({code})"),
        (Some(name), _) => format!("{kind}::{name}"),
        (None, _) => format!("{kind} 0x{}", hex::encode(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_validity_errors() {
        assert_eq!(describe_validity_error(&[0, 1]), "Invalid::Payment");
        assert_eq!(describe_validity_error(&[0, 7, 42]), "Invalid::Custom(42)");
        assert_eq!(
            describe_validity_error(&[1, 1]),
            "Unknown::NoUnsignedValidator"
        );
        assert_eq!(describe_apply_result(&[1, 0, 4]), "Invalid::BadProof");
        assert!(describe_apply_result(&[0, 1, 3, 0]).starts_with("dispatch error"));
    }
}
