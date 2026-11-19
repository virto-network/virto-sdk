//! Full-state snapshots of a live chain at one block.
//!
//! The whole state of a small parachain fits in memory (Kreivo: ~25 MB), so a
//! fork boots from it offline and never merges keys with a remote node.
//! [`Snapshot::download`] only returns a snapshot whose trie reproduces the
//! block's state root; [`Snapshot::verify`] repeats that check on a loaded one.

use std::collections::BTreeMap;
use std::path::Path;

use codec::{Compact, Encode};
use serde_json::{Value, json};
use smoldot::trie::default_child_trie_root_key;

use crate::Hash;
use crate::error::{Error, Result};
use crate::header::Header;
use crate::storage::{Base, Entries};
use crate::trie;
use crate::util::{hex0x, storage_key, twox64, unhex};

const MAGIC: &[u8; 8] = b"SUBEFORK";
const FORMAT_VERSION: u32 = 1;
const KEYS_PAGE: usize = 1000;
const VALUES_BATCH: usize = 1000;

/// A JSON-RPC client to download snapshots with, such as sube's WebSocket
/// backend when its `fork` feature is on.
#[allow(async_fn_in_trait)]
pub trait Transport {
    /// Send a request and return the raw JSON of its result.
    async fn request(&mut self, method: &str, params: &str)
    -> core::result::Result<String, String>;
}

pub struct Snapshot {
    pub chain_name: String,
    pub genesis_hash: Hash,
    /// Raw JSON of `system_properties`, which isn't part of the state.
    pub properties: String,
    pub block_hash: Hash,
    /// SCALE-encoded header of the fork block.
    pub header: Vec<u8>,
    /// Encoded extrinsics of the fork block, with their length prefixes.
    pub extrinsics: Vec<Vec<u8>>,
    pub state: Base,
}

/// Download progress.
#[derive(Debug, Clone, Copy)]
pub enum Progress {
    Keys(usize),
    Values { done: usize, total: usize },
    ChildTries { done: usize, total: usize },
}

fn snapshot_err(e: impl std::fmt::Display) -> Error {
    Error::Snapshot(e.to_string())
}

impl Snapshot {
    /// Download the full state of the finalized block, or of `at`, with the
    /// legacy `state_*` methods full nodes serve, over any [`Transport`]. A pruned node keeps about 256
    /// blocks of state, which leaves a few minutes to finish.
    pub async fn download<T: Transport>(
        rpc: &mut T,
        at: Option<Hash>,
        mut progress: impl FnMut(Progress),
    ) -> Result<Self> {
        let at = match at {
            Some(h) => hex0x(&h),
            None => string(call(rpc, "chain_getFinalizedHead", json!([])).await?)?,
        };
        let (block_hash, header, extrinsics) = fetch_block(rpc, &at).await?;
        let genesis_hash = hash(&call(rpc, "chain_getBlockHash", json!([0])).await?)?;
        let chain_name = string(call(rpc, "system_chain", json!([])).await?)?;
        let properties = call(rpc, "system_properties", json!([])).await?.to_string();
        let main = fetch_main(rpc, &at, &mut progress).await?;
        let child = fetch_children(rpc, &at, &main, &mut progress).await?;
        let snapshot = Snapshot {
            chain_name,
            genesis_hash,
            properties,
            block_hash,
            header,
            extrinsics,
            state: Base { main, child },
        };
        snapshot.verify()?;
        Ok(snapshot)
    }

    pub fn number(&self) -> Option<u32> {
        Header::decode(&mut &self.header[..]).ok().map(|h| h.number)
    }

    /// Check the state reproduces the fork block's state root, child tries
    /// included, and that the genesis hash is the one the runtime signs against.
    pub fn verify(&self) -> Result<()> {
        let header = Header::decode(&mut &self.header[..]).map_err(snapshot_err)?;
        let (root, _) = trie::build(&self.state.main).ok_or_else(|| snapshot_err("empty state"))?;
        if root != header.state_root {
            return Err(snapshot_err(format!(
                "state root mismatch: rebuilt {} but the header has {}",
                hex0x(&root),
                hex0x(&header.state_root)
            )));
        }
        // Child trie roots live in the main trie under their prefixed key.
        for (id, entries) in &self.state.child {
            let expected = self.state.main.get(&default_child_trie_root_key(id));
            let rebuilt = trie::build(entries).map(|(root, _)| root.to_vec());
            if expected != rebuilt.as_ref() {
                return Err(snapshot_err(format!(
                    "child trie {} does not match its root",
                    hex0x(id)
                )));
            }
        }
        // Transactions commit to System::BlockHash(0).
        let genesis_key = [
            storage_key("System", "BlockHash"),
            twox64(&0u32.encode()).to_vec(),
            0u32.encode(),
        ]
        .concat();
        if let Some(stored) = self.state.main.get(&genesis_key)
            && stored[..] != self.genesis_hash[..]
        {
            return Err(snapshot_err(
                "System::BlockHash(0) differs from the genesis hash",
            ));
        }
        Ok(())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = MAGIC.to_vec();
        w.extend(FORMAT_VERSION.to_le_bytes());
        put(&mut w, self.chain_name.as_bytes());
        put(&mut w, &self.genesis_hash);
        put(&mut w, self.properties.as_bytes());
        put(&mut w, &self.block_hash);
        put(&mut w, &self.header);
        put_len(&mut w, self.extrinsics.len());
        for x in &self.extrinsics {
            put(&mut w, x);
        }
        put_entries(&mut w, &self.state.main);
        put_len(&mut w, self.state.child.len());
        for (id, entries) in &self.state.child {
            put(&mut w, id);
            put_entries(&mut w, entries);
        }
        w
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut input = data;
        if take_array::<8>(&mut input) != Some(*MAGIC) {
            return Err(snapshot_err("not a sube-fork snapshot"));
        }
        match take_u32(&mut input) {
            Some(FORMAT_VERSION) => {}
            v => return Err(snapshot_err(format!("unsupported snapshot format {v:?}"))),
        }
        decode_v1(&mut input).ok_or_else(|| snapshot_err("truncated or corrupt snapshot"))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        Ok(std::fs::write(path, self.encode())?)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::decode(&std::fs::read(path)?)
    }
}

async fn fetch_block<T: Transport>(rpc: &mut T, at: &str) -> Result<(Hash, Vec<u8>, Vec<Vec<u8>>)> {
    let block = call(rpc, "chain_getBlock", json!([at])).await?;
    let h = &block["block"]["header"];
    let number = h["number"]
        .as_str()
        .and_then(|n| u32::from_str_radix(n.trim_start_matches("0x"), 16).ok())
        .ok_or_else(|| snapshot_err("bad block number"))?;
    let logs = h["digest"]["logs"]
        .as_array()
        .ok_or_else(|| snapshot_err("no digest logs"))?;
    let mut digest = Compact(logs.len() as u32).encode();
    for log in logs {
        digest.extend(bytes(log)?);
    }
    let header = Header {
        parent: hash(&h["parentHash"])?,
        number,
        state_root: hash(&h["stateRoot"])?,
        extrinsics_root: hash(&h["extrinsicsRoot"])?,
        digest,
    };
    let block_hash = hash(&json!(at))?;
    if header.hash() != block_hash {
        return Err(snapshot_err(
            "the re-encoded header doesn't hash to the block hash",
        ));
    }
    let extrinsics = block["block"]["extrinsics"]
        .as_array()
        .ok_or_else(|| snapshot_err("no extrinsics"))?
        .iter()
        .map(bytes)
        .collect::<Result<_>>()?;
    Ok((block_hash, header.encode(), extrinsics))
}

async fn fetch_main<T: Transport>(
    rpc: &mut T,
    at: &str,
    progress: &mut impl FnMut(Progress),
) -> Result<Entries> {
    let keys = keys_paged(rpc, "state_getKeysPaged", json!(["0x"]), at, |n| {
        progress(Progress::Keys(n))
    })
    .await?;
    let mut main = Entries::new();
    for batch in keys.chunks(VALUES_BATCH) {
        let res = call(rpc, "state_queryStorageAt", json!([batch, at])).await?;
        let changes = res[0]["changes"]
            .as_array()
            .ok_or_else(|| snapshot_err("bad queryStorageAt result"))?;
        for change in changes.iter().filter(|c| c[1].is_string()) {
            main.insert(bytes(&change[0])?, bytes(&change[1])?);
        }
        progress(Progress::Values {
            done: main.len(),
            total: keys.len(),
        });
    }
    if main.len() != keys.len() {
        return Err(snapshot_err(format!(
            "{} keys listed but {} values returned",
            keys.len(),
            main.len()
        )));
    }
    Ok(main)
}

/// Child tries (e.g. contract storage), found by their roots in the main trie.
async fn fetch_children<T: Transport>(
    rpc: &mut T,
    at: &str,
    main: &Entries,
    progress: &mut impl FnMut(Progress),
) -> Result<BTreeMap<Vec<u8>, Entries>> {
    let prefix = default_child_trie_root_key(b"");
    let roots: Vec<&Vec<u8>> = main.keys().filter(|k| k.starts_with(&prefix)).collect();
    let mut child = BTreeMap::new();
    for (i, root) in roots.iter().enumerate() {
        let root_hex = hex0x(root);
        let keys = keys_paged(
            rpc,
            "childstate_getKeysPaged",
            json!([root_hex, "0x"]),
            at,
            |_| {},
        )
        .await?;
        let mut entries = Entries::new();
        for batch in keys.chunks(VALUES_BATCH) {
            let values = call(
                rpc,
                "childstate_getStorageEntries",
                json!([root_hex, batch, at]),
            )
            .await?;
            let values = values
                .as_array()
                .ok_or_else(|| snapshot_err("bad child entries"))?;
            for (k, v) in batch.iter().zip(values).filter(|(_, v)| v.is_string()) {
                entries.insert(
                    unhex(k).ok_or_else(|| snapshot_err("bad child key"))?,
                    bytes(v)?,
                );
            }
        }
        child.insert(root[prefix.len()..].to_vec(), entries);
        progress(Progress::ChildTries {
            done: i + 1,
            total: roots.len(),
        });
    }
    Ok(child)
}

/// Page through `method`, whose params are `leading ++ [count, start, at]`.
async fn keys_paged<T: Transport>(
    rpc: &mut T,
    method: &str,
    leading: Value,
    at: &str,
    mut on_page: impl FnMut(usize),
) -> Result<Vec<String>> {
    let mut keys: Vec<String> = Vec::new();
    loop {
        let mut params = leading.clone();
        let params_list = params.as_array_mut().expect("leading params are an array");
        params_list.extend([json!(KEYS_PAGE), json!(keys.last()), json!(at)]);
        let page: Vec<String> =
            serde_json::from_value(call(rpc, method, params).await?).map_err(snapshot_err)?;
        let full = page.len() == KEYS_PAGE;
        keys.extend(page);
        on_page(keys.len());
        if !full {
            return Ok(keys);
        }
    }
}

fn decode_v1(input: &mut &[u8]) -> Option<Snapshot> {
    let chain_name = String::from_utf8(take(input)?.to_vec()).ok()?;
    let genesis_hash = take(input)?.try_into().ok()?;
    let properties = String::from_utf8(take(input)?.to_vec()).ok()?;
    let block_hash = take(input)?.try_into().ok()?;
    let header = take(input)?.to_vec();
    let extrinsics = (0..take_u32(input)?)
        .map(|_| take(input).map(<[u8]>::to_vec))
        .collect::<Option<_>>()?;
    let main = take_entries(input)?;
    let child = (0..take_u32(input)?)
        .map(|_| Some((take(input)?.to_vec(), take_entries(input)?)))
        .collect::<Option<_>>()?;
    Some(Snapshot {
        chain_name,
        genesis_hash,
        properties,
        block_hash,
        header,
        extrinsics,
        state: Base { main, child },
    })
}

// The file format: little-endian u32 lengths and counts, then raw bytes.

fn put_len(w: &mut Vec<u8>, n: usize) {
    w.extend(
        u32::try_from(n)
            .expect("snapshot sections fit in u32")
            .to_le_bytes(),
    );
}

fn put(w: &mut Vec<u8>, b: &[u8]) {
    put_len(w, b.len());
    w.extend(b);
}

fn put_entries(w: &mut Vec<u8>, entries: &Entries) {
    put_len(w, entries.len());
    for (k, v) in entries {
        put(w, k);
        put(w, v);
    }
}

fn take_array<const N: usize>(input: &mut &[u8]) -> Option<[u8; N]> {
    let (head, rest) = input.split_first_chunk::<N>()?;
    *input = rest;
    Some(*head)
}

fn take_u32(input: &mut &[u8]) -> Option<u32> {
    take_array(input).map(u32::from_le_bytes)
}

fn take<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    let n = take_u32(input)? as usize;
    let (head, rest) = input.split_at_checked(n)?;
    *input = rest;
    Some(head)
}

fn take_entries(input: &mut &[u8]) -> Option<Entries> {
    (0..take_u32(input)?)
        .map(|_| Some((take(input)?.to_vec(), take(input)?.to_vec())))
        .collect()
}

async fn call<T: Transport>(rpc: &mut T, method: &str, params: Value) -> Result<Value> {
    let raw = rpc
        .request(method, &params.to_string())
        .await
        .map_err(|e| snapshot_err(format!("{method}: {e}")))?;
    serde_json::from_str(&raw).map_err(|e| snapshot_err(format!("{method}: {e}")))
}

fn string(v: Value) -> Result<String> {
    v.as_str()
        .map(Into::into)
        .ok_or_else(|| snapshot_err(format!("expected a string, got {v}")))
}

fn bytes(v: &Value) -> Result<Vec<u8>> {
    v.as_str()
        .and_then(unhex)
        .ok_or_else(|| snapshot_err(format!("expected hex, got {v}")))
}

fn hash(v: &Value) -> Result<Hash> {
    bytes(v)?
        .try_into()
        .map_err(|_| snapshot_err(format!("expected a 32 byte hash, got {v}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(main: Entries, child: BTreeMap<Vec<u8>, Entries>) -> Snapshot {
        let state_root = trie::build(&main).map(|(r, _)| r).unwrap_or_default();
        let header = Header {
            parent: [7; 32],
            number: 5,
            state_root,
            extrinsics_root: [0; 32],
            digest: vec![0],
        };
        Snapshot {
            chain_name: "Test".into(),
            genesis_hash: [1; 32],
            properties: r#"{"ss58Format":2}"#.into(),
            block_hash: header.hash(),
            header: header.encode(),
            extrinsics: vec![vec![4, 1, 2]],
            state: Base { main, child },
        }
    }

    #[test]
    fn encode_roundtrip() {
        let kid: Entries = [(b"a".to_vec(), b"b".to_vec())].into();
        let main: Entries = [
            (b"k".to_vec(), b"v".to_vec()),
            (
                default_child_trie_root_key(b"id"),
                trie::build(&kid).unwrap().0.to_vec(),
            ),
        ]
        .into();
        let snap = snapshot(main, [(b"id".to_vec(), kid)].into());
        snap.verify().unwrap();

        let back = Snapshot::decode(&snap.encode()).unwrap();
        assert_eq!(back.chain_name, "Test");
        assert_eq!(back.number(), Some(5));
        assert_eq!(back.extrinsics, snap.extrinsics);
        assert_eq!(back.state.main, snap.state.main);
        assert_eq!(back.state.child, snap.state.child);
        assert_eq!(back.properties, snap.properties);

        let mut tampered = back;
        tampered
            .state
            .child
            .get_mut(&b"id"[..])
            .unwrap()
            .insert(b"z".to_vec(), b"x".to_vec());
        assert!(
            tampered
                .verify()
                .unwrap_err()
                .to_string()
                .contains("child trie")
        );
    }

    #[test]
    fn rejects_a_wrong_state_root() {
        let mut snap = snapshot([(b"k".to_vec(), b"v".to_vec())].into(), BTreeMap::new());
        snap.state.main.insert(b"extra".to_vec(), b"x".to_vec());
        assert!(
            snap.verify()
                .unwrap_err()
                .to_string()
                .contains("state root mismatch")
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(Snapshot::decode(b"nope").is_err());
        let mut truncated =
            snapshot([(b"k".to_vec(), b"v".to_vec())].into(), BTreeMap::new()).encode();
        truncated.truncate(truncated.len() - 1);
        assert!(Snapshot::decode(&truncated).is_err());
    }
}
