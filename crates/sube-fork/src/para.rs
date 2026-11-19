//! Everything that assumes a cumulus parachain lives here.
//!
//! The relay chain is mocked by rewriting the fork block's real
//! `set_validation_data` inherent for every new block:
//! - the relay state entries in its proof are kept (host configuration,
//!   messaging state, BABE authorities, upgrade signals...), and only the relay
//!   slot and our included para head are changed;
//! - the proof is rebuilt as a complete trie over those entries, so its root is
//!   not the relay chain's but is consistent;
//! - the relay parent and its descendant headers are re-chained onto that root;
//!   their seals no longer match, so the executor trusts the BABE authorities
//!   found in the proof (see `SignaturePolicy::trusted_keys`);
//! - inbound XCM messages are dropped.
//!
//! The relay chain block number doubles as the clock of runtimes (like
//! Kreivo's) whose pallets use `RelaychainDataProvider`; time travel moves it.
//!
//! Assumptions, checked by a dry-run block when a fork boots: the fork block's
//! first extrinsic is `ParachainSystem::set_validation_data` and its second is
//! `Timestamp::set`; para and relay slots are both 6s, so they are equal; and
//! the runtime only needs an Aura pre-runtime digest.

use std::collections::BTreeMap;

use codec::{Compact, Decode, Encode};
use smoldot::trie::{
    nibbles_to_bytes_truncate,
    proof_decode::{self, StorageValue as ProofValue},
};

use crate::error::{Error, Result};
use crate::header::Header;
use crate::trie;
use crate::util::{storage_key, twox64};

pub const SLOT_DURATION_MS: u64 = 6_000;
/// Consensus engine of the para block's pre-runtime slot digest.
pub const AURA_ENGINE: &[u8; 4] = b"aura";

/// Mock relay chain state, advanced by one or more relay blocks per para block.
#[derive(Clone, Debug)]
pub struct Relay {
    /// Extrinsic format byte of the live bare inherents (v4 or v5).
    version: u8,
    /// Call indices of `set_validation_data` and `Timestamp::set`.
    validation_call: [u8; 2],
    timestamp_call: [u8; 2],
    entries: BTreeMap<Vec<u8>, Vec<u8>>,
    pub number: u32,
    pub slot: u64,
    max_pov: u32,
    /// The relay parent followed by its descendants.
    descendants: Vec<Header>,
    collator_peer_id: Option<Vec<u8>>,
    para_id: u32,
}

/// The inherents of one para block, encoded as extrinsics.
pub struct Inherents {
    pub validation_data: Vec<u8>,
    pub timestamp: Vec<u8>,
    pub slot: u64,
}

fn layout(what: &str) -> impl FnOnce(codec::Error) -> Error {
    move |e| Error::Layout(format!("{what}: {e}"))
}

fn babe_slot_key() -> Vec<u8> {
    storage_key("Babe", "CurrentSlot")
}

fn para_head_key(para_id: u32) -> Vec<u8> {
    let id = para_id.encode();
    [storage_key("Paras", "Heads"), twox64(&id).to_vec(), id].concat()
}

/// The format byte and call of a bare (unsigned) extrinsic.
fn bare_call(ext: &[u8]) -> Result<(u8, &[u8])> {
    let mut input = ext;
    let len = Compact::<u32>::decode(&mut input)
        .map_err(layout("extrinsic"))?
        .0 as usize;
    match input.get(..len).and_then(<[u8]>::split_first) {
        // Signed extrinsics set the top bit of the format byte.
        Some((version, call)) if version & 0x80 == 0 => Ok((*version, call)),
        _ => Err(Error::Layout("expected a bare inherent".into())),
    }
}

/// Encode `call` as a bare extrinsic.
fn bare(version: u8, call: &[u8]) -> Vec<u8> {
    [&[version][..], call].concat().encode()
}

impl Relay {
    /// Read the relay chain context from the fork block's inherents.
    pub fn from_block(extrinsics: &[Vec<u8>], para_id: u32) -> Result<Self> {
        let [validation, timestamp, ..] = extrinsics else {
            return Err(Error::Layout("the fork block has no inherents".into()));
        };
        let (version, mut call) = bare_call(validation)?;
        let (_, timestamp_call) = bare_call(timestamp)?;
        let timestamp_call =
            <[u8; 2]>::decode(&mut &timestamp_call[..]).map_err(layout("timestamp call"))?;

        let input = &mut call;
        let validation_call = <[u8; 2]>::decode(input).map_err(layout("call index"))?;
        Vec::<u8>::decode(input).map_err(layout("parent head"))?;
        let number = u32::decode(input).map_err(layout("relay parent number"))?;
        let root = <[u8; 32]>::decode(input).map_err(layout("relay parent storage root"))?;
        let max_pov = u32::decode(input).map_err(layout("max pov size"))?;
        let proof = Vec::<Vec<u8>>::decode(input).map_err(layout("relay state proof"))?;
        let descendants = (0..Compact::<u32>::decode(input)
            .map_err(layout("descendants"))?
            .0)
            .map(|_| Header::decode(input).map_err(layout("descendant header")))
            .collect::<Result<_>>()?;
        let collator_peer_id =
            Option::<Vec<u8>>::decode(input).map_err(layout("collator peer id"))?;

        let proof = trie::encode_proof(&proof);
        let decoded =
            proof_decode::decode_and_verify_proof(proof_decode::Config { proof: &proof[..] })
                .map_err(|e| Error::Layout(format!("relay state proof: {e}")))?;
        let mut entries = BTreeMap::new();
        for (key, entry) in decoded.iter_ordered() {
            let nibbles: Vec<_> = key.key.collect();
            if *key.trie_root_hash == root
                && nibbles.len().is_multiple_of(2)
                && let ProofValue::Known { value, .. } = entry.trie_node_info.storage_value
            {
                entries.insert(
                    nibbles_to_bytes_truncate(nibbles.into_iter()).collect(),
                    value.to_vec(),
                );
            }
        }

        let slot = entries
            .get(&babe_slot_key())
            .and_then(|v| u64::decode(&mut &v[..]).ok())
            .ok_or_else(|| {
                Error::Layout("the relay state proof has no Babe::CurrentSlot".into())
            })?;
        Ok(Relay {
            version,
            validation_call,
            timestamp_call,
            entries,
            number,
            slot,
            max_pov,
            descendants,
            collator_peer_id,
            para_id,
        })
    }

    /// BABE authorities (current and next) in the relay state proof.
    pub fn babe_authorities(&self) -> Vec<[u8; 32]> {
        ["Authorities", "NextAuthorities"]
            .into_iter()
            .filter_map(|item| self.entries.get(&storage_key("Babe", item)))
            .filter_map(|v| Vec::<([u8; 32], u64)>::decode(&mut &v[..]).ok())
            .flatten()
            .map(|(key, _weight)| key)
            .collect()
    }

    /// Timestamp of the current relay slot, which is also the para slot.
    pub fn timestamp(&self) -> u64 {
        self.slot * SLOT_DURATION_MS
    }

    /// Advance the relay chain by `relay_blocks`, one slot each, and return the
    /// inherents of a para block on top of `parent_header`.
    pub fn advance(&mut self, parent_header: &[u8], relay_blocks: u32) -> Inherents {
        self.number += relay_blocks;
        self.slot += u64::from(relay_blocks);
        self.entries.insert(babe_slot_key(), self.slot.encode());
        // Everything up to our parent counts as included: the unincluded
        // segment drains and the consensus hook finds the parent included.
        self.entries
            .insert(para_head_key(self.para_id), parent_header.encode());
        let (root, nodes) = trie::build(&self.entries).expect("the relay proof is never empty");

        let mut parent = None;
        for (i, h) in self.descendants.iter_mut().enumerate() {
            h.number = self.number + i as u32;
            match parent {
                None => h.state_root = root,
                Some(hash) => h.parent = hash,
            }
            parent = Some(h.hash());
        }

        let mut call = self.validation_call.to_vec();
        parent_header.encode_to(&mut call);
        self.number.encode_to(&mut call);
        call.extend(root);
        self.max_pov.encode_to(&mut call);
        nodes.encode_to(&mut call);
        Compact(self.descendants.len() as u32).encode_to(&mut call);
        for h in &self.descendants {
            call.extend(h.encode());
        }
        self.collator_peer_id.encode_to(&mut call);
        // InboundMessagesData: no downward and no horizontal messages, each an
        // empty (full_messages, hashed_messages) pair.
        call.extend([0u8; 4]);

        let timestamp = [
            &self.timestamp_call[..],
            &Compact(self.timestamp()).encode(),
        ]
        .concat();
        Inherents {
            validation_data: bare(self.version, &call),
            timestamp: bare(self.version, &timestamp),
            slot: self.slot,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inherents of Kreivo block #41128860.
    const KREIVO_INHERENTS: &[u8] = include_bytes!("../tests/fixtures/kreivo-inherents.scale");
    const KREIVO_PARA_ID: u32 = 2281;

    fn relay() -> Relay {
        let extrinsics = Vec::<Vec<u8>>::decode(&mut &KREIVO_INHERENTS[..]).unwrap();
        Relay::from_block(&extrinsics, KREIVO_PARA_ID).unwrap()
    }

    #[test]
    fn reads_the_live_inherent() {
        let relay = relay();
        assert_eq!(
            relay.descendants.len(),
            2,
            "Kreivo's relay parent offset is 1"
        );
        assert!(!relay.babe_authorities().is_empty());
        assert!(relay.entries.len() > 40);
    }

    #[test]
    fn rewritten_inherent_reads_back() {
        let mut relay = relay();
        let parent = Header::pre_runtime([9; 32], 41_128_861, b"aura", relay.slot + 1).encode();
        let inherents = relay.advance(&parent, 3);

        let again = Relay::from_block(
            &[inherents.validation_data, inherents.timestamp],
            KREIVO_PARA_ID,
        )
        .expect("the forged proof verifies against its root");
        assert_eq!((again.number, again.slot), (relay.number, relay.slot));
        assert_eq!(
            again.entries[&para_head_key(KREIVO_PARA_ID)],
            parent.encode()
        );
        assert_eq!(again.descendants[1].parent, again.descendants[0].hash());
        assert_eq!(again.descendants[0].number, relay.number);
        assert_eq!(again.babe_authorities(), relay.babe_authorities());
    }
}
