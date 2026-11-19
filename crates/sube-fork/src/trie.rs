//! Build a complete base-16 Merkle-Patricia trie (state version 1, blake2)
//! from a set of entries, returning its root and every node needed to prove them.
//!
//! The relay chain mock uses it to forge the state proof in the parachain
//! inherent, and snapshots use it to check they reproduce the block's state root.

use std::collections::BTreeMap;

use smoldot::trie::{
    Nibble, bytes_to_nibbles,
    trie_node::{self, Decoded, StorageValue},
};

use crate::Hash;
use crate::util::blake2_256;

/// Values of this length or more are stored hashed (state version 1).
const V1_HASHED_VALUE_MIN: usize = 33;

/// Root and deduplicated proof nodes (hashed values included) of the trie
/// holding exactly `entries`, or `None` for an empty map.
pub fn build(entries: &BTreeMap<Vec<u8>, Vec<u8>>) -> Option<(Hash, Vec<Vec<u8>>)> {
    if entries.is_empty() {
        return None;
    }
    let items: Vec<(Vec<Nibble>, &[u8])> = entries
        .iter()
        .map(|(k, v)| (bytes_to_nibbles(k.iter().copied()).collect(), v.as_slice()))
        .collect();
    let mut nodes = Vec::new();
    let root = node(&items, 0, true, &mut nodes);
    // Identical subtrees or values hash the same; a proof is a set.
    nodes.sort();
    nodes.dedup();
    Some((
        root.try_into().expect("the root's merkle value is a hash"),
        nodes,
    ))
}

/// SCALE-encode proof nodes as `Vec<Vec<u8>>` (`sp_trie::StorageProof`).
pub fn encode_proof(nodes: &[Vec<u8>]) -> Vec<u8> {
    codec::Encode::encode(nodes)
}

/// Encode the node covering `items`, which are sorted and share `depth`
/// leading nibbles, and return its Merkle value: the hash of the node, or
/// the node itself when shorter than 32 bytes.
fn node(
    items: &[(Vec<Nibble>, &[u8])],
    depth: usize,
    is_root: bool,
    nodes: &mut Vec<Vec<u8>>,
) -> Vec<u8> {
    // The keys are sorted, so the prefix all of them share is the one the
    // first and last share.
    let (first, last) = (&items[0].0, &items[items.len() - 1].0);
    let end = depth
        + first[depth..]
            .iter()
            .zip(&last[depth..])
            .take_while(|(a, b)| a == b)
            .count();
    let (value, rest) = if first.len() == end {
        (Some(items[0].1), &items[1..])
    } else {
        (None, items)
    };

    let mut children: [Option<Vec<u8>>; 16] = Default::default();
    for group in rest.chunk_by(|a, b| a.0[end] == b.0[end]) {
        let nibble = usize::from(u8::from(group[0].0[end]));
        children[nibble] = Some(node(group, end + 1, false, nodes));
    }

    let value_hash;
    let storage_value = match value {
        Some(v) if v.len() >= V1_HASHED_VALUE_MIN => {
            value_hash = blake2_256(v);
            nodes.push(v.to_vec());
            StorageValue::Hashed(&value_hash)
        }
        Some(v) => StorageValue::Unhashed(v),
        None => StorageValue::None,
    };
    let encoded = trie_node::encode_to_vec(Decoded {
        partial_key: first[depth..end].iter().copied(),
        children,
        storage_value,
    })
    .expect("a node with a value or two children is well-formed");

    if is_root || encoded.len() >= 32 {
        let hash = blake2_256(&encoded);
        nodes.push(encoded);
        hash.to_vec()
    } else {
        encoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smoldot::trie::{
        HashFunction, TrieEntryVersion,
        calculate_root::{self, RootMerkleValueCalculation as Calc},
        proof_decode,
    };
    use std::ops::Bound;

    /// The root as computed by smoldot's root calculation, driven as in its
    /// documentation. smoldot's `trie_root` helper gives a different root for
    /// this sample, while `build` agrees with this driver and with real Kreivo
    /// state roots.
    fn reference_root(entries: &BTreeMap<Vec<u8>, Vec<u8>>) -> Hash {
        let mut calc = calculate_root::root_merkle_value(HashFunction::Blake2);
        loop {
            calc = match calc {
                Calc::Finished { hash, .. } => return hash,
                Calc::StorageValue(req) => {
                    let value = entries.get(&req.key().collect::<Vec<_>>());
                    req.inject(value.map(|v| (v, TrieEntryVersion::V1)))
                }
                Calc::NextKey(req) => {
                    let before: Vec<u8> = req.key_before().collect();
                    let prefix: Vec<u8> = req.prefix().collect();
                    let lower = if req.or_equal() {
                        Bound::Included(before)
                    } else {
                        Bound::Excluded(before)
                    };
                    let found = entries
                        .range((lower, Bound::Unbounded))
                        .map(|(k, _)| k)
                        .next()
                        .filter(|k| k.starts_with(&prefix));
                    req.inject_key(found.map(|k| k.iter().copied()))
                }
            }
        }
    }

    fn sample() -> BTreeMap<Vec<u8>, Vec<u8>> {
        BTreeMap::from([
            (b"foo".to_vec(), b"bar".to_vec()),
            (b"food".to_vec(), vec![7; 40]), // hashed value
            (b"fox".to_vec(), vec![1]),
            (vec![0x00], vec![2; 33]),
            (vec![0x00, 0x01], vec![3]),
            (vec![0xff; 32], vec![4; 100]),
            (vec![0xff; 31], vec![4; 100]), // the same value twice: deduplicated
        ])
    }

    #[test]
    fn root_matches_smoldot() {
        for n in 1..=sample().len() {
            let entries: BTreeMap<_, _> = sample().into_iter().take(n).collect();
            assert_eq!(
                build(&entries).unwrap().0,
                reference_root(&entries),
                "{n} entries"
            );
        }
    }

    #[test]
    fn proof_verifies_and_proves_every_entry() {
        let entries = sample();
        let (root, nodes) = build(&entries).unwrap();
        let proof = encode_proof(&nodes);
        let decoded =
            proof_decode::decode_and_verify_proof(proof_decode::Config { proof: &proof[..] })
                .expect("valid proof");
        for (k, v) in &entries {
            let value = decoded
                .storage_value(&root, k)
                .expect("key is in the proof");
            assert_eq!(value.map(|(value, _)| value), Some(&v[..]));
        }
    }

    #[test]
    fn empty_has_no_root() {
        assert!(build(&BTreeMap::new()).is_none());
    }
}
