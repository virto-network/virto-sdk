//! ChainHead v1 session manager.
//!
//! Manages a `chainHead_v1_follow` subscription and provides high-level
//! methods for storage queries, runtime calls, and block tracking.
//!
//! Pin strategy: we only keep the latest finalized block pinned.
//! Between operations we drain queued events and unpin everything
//! except the current finalized block.

use alloc::{collections::BTreeMap, collections::VecDeque, format, string::String, vec::Vec};

use codec::Decode;

use super::{
    Rpc, RpcSubscription, extract_json_object, extract_json_str, extract_json_string,
    result_as_str, to_hex,
};
use crate::meta::{self, Metadata};
use crate::prelude::*;

/// A chain event visible to users — new blocks, finalization, best block changes.
#[derive(Debug, Clone)]
pub enum ChainEvent {
    /// A new block was imported.
    NewBlock {
        hash: String,
        parent: String,
        number: u64,
        /// True if the runtime was upgraded in this block.
        is_new_runtime: bool,
    },
    /// The best (head) block changed.
    BestBlock { hash: String },
    /// One or more blocks were finalized.
    Finalized {
        /// Block hashes that were finalized (in order).
        hashes: Vec<String>,
        /// Block hashes that were pruned (fork branches).
        pruned: Vec<String>,
    },
}

/// Decoded block header fields.
#[derive(Debug, Clone)]
pub struct BlockHeader {
    pub parent_hash: String,
    pub number: u64,
    pub state_root: String,
    pub extrinsics_root: String,
}

/// A chainHead session that manages a `chainHead_v1_follow` subscription.
pub struct ChainHead<R> {
    rpc: R,
    follow_sub_id: String,
    finalized_hash: String,
    genesis_hash: [u8; 32],
    /// Accumulates storage items for in-progress operations.
    storage_accum: BTreeMap<String, Vec<StorageItem>>,
    /// Block hashes that are pinned on the server but we don't need.
    /// Unpinned lazily in bulk before each operation.
    pending_unpin: Vec<String>,
    /// Set when a finalized event arrives after flush — means finalized_hash
    /// points to an already-unpinned block and we need to refollow.
    needs_refollow: bool,
    /// Explicit snapshot leases held by higher-level paged queries. Leased
    /// hashes are excluded from lazy unpin batches until released.
    retained_hashes: BTreeMap<String, usize>,
    /// User-visible events buffered during internal operations.
    event_queue: VecDeque<ChainEvent>,
    /// Operation whose owning future is currently waiting for follow events.
    /// It deliberately remains set when that future is cancelled.
    active_operation: Option<String>,
    /// Transaction-watch subscription retained across caller cancellation so
    /// a host-level deadline can still unwatch it explicitly.
    active_tx_watch: Option<String>,
}

/// Result from a chainHead operation.
enum OperationResult {
    StorageItems(Vec<StorageItem>),
    CallDone(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct StorageItem {
    pub key: String,
    pub value: Option<String>,
    pub hash: Option<String>,
    pub closest_descendant_merkle_value: Option<String>,
}

const LIGHT_PAGE_CURSOR_MAGIC: &[u8; 8] = b"SUBEPG01";
const LIGHT_PAGE_MAX_DEPTH: usize = 8;
const LIGHT_PAGE_MAX_OPERATIONS: usize = 64;

#[derive(Debug)]
enum KeyOperation {
    Complete(Vec<crate::RawKey>),
    Split { exact_keys: Vec<crate::RawKey> },
}

#[derive(Debug)]
enum KeyItemOperation {
    Complete(Vec<crate::RawKey>),
    TooLarge,
    Inaccessible,
}

// --- Follow event types ---

/// Parsed follow event — borrows string values from the JSON input.
/// Consumed immediately; values that must outlive the JSON are cloned at the call site.
#[derive(Debug)]
enum FollowEvent<'a> {
    Initialized {
        finalized_block_hashes: Vec<&'a str>,
    },
    NewBlock {
        block_hash: &'a str,
        parent_block_hash: &'a str,
        has_new_runtime: bool,
    },
    BestBlockChanged {
        best_block_hash: &'a str,
    },
    Finalized {
        finalized_block_hashes: Vec<&'a str>,
        pruned_block_hashes: Vec<&'a str>,
    },
    Stop,
    OperationCallDone {
        operation_id: &'a str,
        output: &'a str,
    },
    OperationStorageItems {
        operation_id: &'a str,
        items: Vec<StorageItem>,
    },
    OperationStorageDone {
        operation_id: &'a str,
    },
    OperationError {
        operation_id: &'a str,
        error: &'a str,
    },
    OperationInaccessible {
        operation_id: &'a str,
    },
    OperationWaitingForContinue {
        operation_id: &'a str,
    },
}

/// Extract a JSON array of quoted strings as borrowed slices.
///
/// Finds `marker` in `json`, parses the `[...]` that follows, and returns
/// each quoted element as a `&str` borrowing from `json`. Zero allocation
/// for the strings themselves. Safe for hex hashes and identifiers.
fn extract_str_array<'a>(json: &'a str, marker: &str) -> Vec<&'a str> {
    let marker_pos = match json.find(marker) {
        Some(p) => p + marker.len(),
        None => return Vec::new(),
    };
    let rest = json[marker_pos..].trim_start();
    if !rest.starts_with('[') {
        return Vec::new();
    }
    // Find the offset of the array content within the original json
    let arr_start = json.len() - rest.len();
    // Find matching `]`
    let mut depth = 0;
    let mut arr_end = arr_start;
    for (i, ch) in rest.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    arr_end = arr_start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let inner = &json[arr_start + 1..arr_end];
    let mut result = Vec::new();
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b',' | b'\n' | b'\r' | b'\t' => i += 1,
            b'"' => {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += 1;
                }
                result.push(&inner[start..i]);
                if i < bytes.len() {
                    i += 1;
                }
            }
            _ => break,
        }
    }
    result
}

/// Deserialize storage items from a JSON `"items":[...]` array.
///
/// Each item carries a key and one of value/hash/closest-descendant fields.
fn parse_storage_items(json: &str) -> Vec<StorageItem> {
    let arr = match extract_json_object(json, "\"items\":") {
        Some(a) => a,
        None => return Vec::new(),
    };
    let arr = arr.trim();
    if !arr.starts_with('[') || !arr.ends_with(']') {
        return Vec::new();
    }
    // Each item is a simple flat object with "key" and optional "value".
    // We find each {...} and extract fields with the standard helper.
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, ch) in arr.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let obj = &arr[start..=i];
                    let key = extract_json_string(obj, "\"key\":\"").unwrap_or_default();
                    let value = extract_json_string(obj, "\"value\":\"");
                    let hash = extract_json_string(obj, "\"hash\":\"");
                    let closest_descendant_merkle_value =
                        extract_json_string(obj, "\"closestDescendantMerkleValue\":\"");
                    items.push(StorageItem {
                        key,
                        value,
                        hash,
                        closest_descendant_merkle_value,
                    });
                }
            }
            _ => {}
        }
    }
    items
}

fn encode_light_page_cursor(
    partition: &[u8],
    after_key: Option<&[u8]>,
) -> crate::Result<crate::RawKey> {
    let partition_len = u8::try_from(partition.len()).map_err(|_| crate::Error::BadInput)?;
    let after_key = after_key.unwrap_or_default();
    let after_len = u16::try_from(after_key.len()).map_err(|_| crate::Error::BadInput)?;
    let mut cursor = Vec::with_capacity(
        LIGHT_PAGE_CURSOR_MAGIC.len() + 1 + partition.len() + 2 + after_key.len(),
    );
    cursor.extend_from_slice(LIGHT_PAGE_CURSOR_MAGIC);
    cursor.push(partition_len);
    cursor.extend_from_slice(partition);
    cursor.extend_from_slice(&after_len.to_le_bytes());
    cursor.extend_from_slice(after_key);
    Ok(cursor)
}

fn decode_light_page_cursor(
    prefix: &[u8],
    cursor: Option<crate::RawKey>,
) -> crate::Result<(Vec<u8>, Option<crate::RawKey>)> {
    let Some(cursor) = cursor else {
        return Ok((Vec::new(), None));
    };

    if !cursor.starts_with(LIGHT_PAGE_CURSOR_MAGIC) {
        if !cursor.starts_with(prefix) {
            return Err(crate::Error::BadInput);
        }
        let suffix = &cursor[prefix.len()..];
        if suffix.is_empty() {
            return Ok((vec![0], None));
        }
        let depth = core::cmp::min(LIGHT_PAGE_MAX_DEPTH, suffix.len());
        return Ok((suffix[..depth].to_vec(), Some(cursor)));
    }

    let partition_len = usize::from(
        *cursor
            .get(LIGHT_PAGE_CURSOR_MAGIC.len())
            .ok_or(crate::Error::BadInput)?,
    );
    if partition_len > LIGHT_PAGE_MAX_DEPTH {
        return Err(crate::Error::BadInput);
    }
    let partition_start = LIGHT_PAGE_CURSOR_MAGIC.len() + 1;
    let partition_end = partition_start
        .checked_add(partition_len)
        .ok_or(crate::Error::BadInput)?;
    let after_len_end = partition_end.checked_add(2).ok_or(crate::Error::BadInput)?;
    let partition = cursor
        .get(partition_start..partition_end)
        .ok_or(crate::Error::BadInput)?
        .to_vec();
    let after_len = u16::from_le_bytes(
        cursor
            .get(partition_end..after_len_end)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(crate::Error::BadInput)?,
    ) as usize;
    let after_end = after_len_end
        .checked_add(after_len)
        .ok_or(crate::Error::BadInput)?;
    if after_end != cursor.len() {
        return Err(crate::Error::BadInput);
    }
    let after_key = if after_len == 0 {
        None
    } else {
        let key = cursor[after_len_end..after_end].to_vec();
        let mut expected_prefix = Vec::with_capacity(prefix.len() + partition.len());
        expected_prefix.extend_from_slice(prefix);
        expected_prefix.extend_from_slice(&partition);
        if !key.starts_with(&expected_prefix) {
            return Err(crate::Error::BadInput);
        }
        Some(key)
    };
    Ok((partition, after_key))
}

/// Advance past an entirely scanned prefix subtree. Truncating at the byte
/// that carried preserves lexicographic depth-first traversal.
fn advance_light_page_partition(partition: &mut Vec<u8>) -> bool {
    for index in (0..partition.len()).rev() {
        if partition[index] != u8::MAX {
            partition[index] += 1;
            partition.truncate(index + 1);
            return true;
        }
    }
    false
}

fn parse_follow_event(json: &str) -> Result<FollowEvent<'_>, crate::Error> {
    let event = extract_json_str(json, "\"event\":\"")
        .ok_or_else(|| crate::Error::Decode("missing event field".into()))?;
    match event {
        "initialized" => Ok(FollowEvent::Initialized {
            finalized_block_hashes: extract_str_array(json, "\"finalizedBlockHashes\":"),
        }),
        "newBlock" => Ok(FollowEvent::NewBlock {
            block_hash: extract_json_str(json, "\"blockHash\":\"")
                .ok_or_else(|| crate::Error::Decode("missing blockHash".into()))?,
            parent_block_hash: extract_json_str(json, "\"parentBlockHash\":\"")
                .ok_or_else(|| crate::Error::Decode("missing parentBlockHash".into()))?,
            has_new_runtime: json.contains("\"newRuntime\":")
                && !json.contains("\"newRuntime\":null"),
        }),
        "bestBlockChanged" => Ok(FollowEvent::BestBlockChanged {
            best_block_hash: extract_json_str(json, "\"bestBlockHash\":\"")
                .ok_or_else(|| crate::Error::Decode("missing bestBlockHash".into()))?,
        }),
        "finalized" => Ok(FollowEvent::Finalized {
            finalized_block_hashes: extract_str_array(json, "\"finalizedBlockHashes\":"),
            pruned_block_hashes: extract_str_array(json, "\"prunedBlockHashes\":"),
        }),
        "stop" => Ok(FollowEvent::Stop),
        "operationCallDone" => Ok(FollowEvent::OperationCallDone {
            operation_id: extract_json_str(json, "\"operationId\":\"")
                .ok_or_else(|| crate::Error::Decode("missing operationId".into()))?,
            output: extract_json_str(json, "\"output\":\"")
                .ok_or_else(|| crate::Error::Decode("missing output".into()))?,
        }),
        "operationStorageItems" => Ok(FollowEvent::OperationStorageItems {
            operation_id: extract_json_str(json, "\"operationId\":\"")
                .ok_or_else(|| crate::Error::Decode("missing operationId".into()))?,
            items: parse_storage_items(json),
        }),
        "operationStorageDone" => Ok(FollowEvent::OperationStorageDone {
            operation_id: extract_json_str(json, "\"operationId\":\"")
                .ok_or_else(|| crate::Error::Decode("missing operationId".into()))?,
        }),
        "operationError" => Ok(FollowEvent::OperationError {
            operation_id: extract_json_str(json, "\"operationId\":\"")
                .ok_or_else(|| crate::Error::Decode("missing operationId".into()))?,
            error: extract_json_str(json, "\"error\":\"").unwrap_or("unknown error"),
        }),
        "operationInaccessible" => Ok(FollowEvent::OperationInaccessible {
            operation_id: extract_json_str(json, "\"operationId\":\"")
                .ok_or_else(|| crate::Error::Decode("missing operationId".into()))?,
        }),
        "operationWaitingForContinue" => Ok(FollowEvent::OperationWaitingForContinue {
            operation_id: extract_json_str(json, "\"operationId\":\"")
                .ok_or_else(|| crate::Error::Decode("missing operationId".into()))?,
        }),
        other => Err(crate::Error::Decode(format!(
            "unknown follow event: {other}"
        ))),
    }
}

#[derive(Debug)]
enum OperationStarted<'a> {
    Started {
        operation_id: &'a str,
        discarded_items: usize,
    },
    LimitReached,
}

fn extract_json_usize(json: &str, marker: &str) -> Option<usize> {
    let tail = json.get(json.find(marker)? + marker.len()..)?;
    let digits = tail
        .trim_start()
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (digits != 0)
        .then(|| tail.trim_start().get(..digits)?.parse().ok())
        .flatten()
}

fn parse_operation_started(json: &str) -> Result<OperationStarted<'_>, crate::Error> {
    let result = extract_json_str(json, "\"result\":\"")
        .ok_or_else(|| crate::Error::Decode("missing result in operation response".into()))?;
    match result {
        "started" => {
            let discarded_items = if json.contains("\"discardedItems\":") {
                extract_json_usize(json, "\"discardedItems\":").ok_or_else(|| {
                    crate::Error::Decode("invalid discardedItems in operation response".into())
                })?
            } else {
                0
            };
            Ok(OperationStarted::Started {
                operation_id: extract_json_str(json, "\"operationId\":\"")
                    .ok_or_else(|| crate::Error::Decode("missing operationId".into()))?,
                discarded_items,
            })
        }
        "limitReached" => Ok(OperationStarted::LimitReached),
        other => Err(crate::Error::Decode(format!(
            "unknown operation result: {other}"
        ))),
    }
}

// --- Archive storage event types ---

#[derive(Debug)]
enum ArchiveStorageEvent<'a> {
    Items { items: Vec<StorageItem> },
    Done,
    Error { error: &'a str },
    WaitingForContinue,
}

fn parse_archive_storage_event(json: &str) -> Result<ArchiveStorageEvent<'_>, crate::Error> {
    let event = extract_json_str(json, "\"event\":\"")
        .ok_or_else(|| crate::Error::Decode("missing event field".into()))?;
    match event {
        "items" => Ok(ArchiveStorageEvent::Items {
            items: parse_storage_items(json),
        }),
        "done" => Ok(ArchiveStorageEvent::Done),
        "error" => Ok(ArchiveStorageEvent::Error {
            error: extract_json_str(json, "\"error\":\"").unwrap_or("unknown error"),
        }),
        "waitingForContinue" => Ok(ArchiveStorageEvent::WaitingForContinue),
        other => Err(crate::Error::Decode(format!(
            "unknown archive event: {other}"
        ))),
    }
}

// --- Transaction watch event types ---

#[derive(Debug, Clone)]
struct TxBlock {
    hash: String,
    index: u32,
}

#[derive(Debug)]
#[allow(dead_code)]
enum TxEvent<'a> {
    Validated,
    Broadcasted,
    BestChainBlockIncluded { block: Option<TxBlock> },
    Finalized { block: TxBlock },
    Invalid { error: &'a str },
    Dropped { error: &'a str },
    Error { error: &'a str },
}

fn parse_tx_event(json: &str) -> Result<TxEvent<'_>, crate::Error> {
    let event = extract_json_str(json, "\"event\":\"")
        .ok_or_else(|| crate::Error::Decode("missing event field".into()))?;
    match event {
        "validated" => Ok(TxEvent::Validated),
        "broadcasted" => Ok(TxEvent::Broadcasted),
        "bestChainBlockIncluded" => Ok(TxEvent::BestChainBlockIncluded {
            block: parse_tx_block(json),
        }),
        "finalized" => Ok(TxEvent::Finalized {
            block: parse_tx_block(json)
                .ok_or_else(|| crate::Error::Decode("finalized event has no block".into()))?,
        }),
        "invalid" => Ok(TxEvent::Invalid {
            error: extract_json_str(json, "\"error\":\"").unwrap_or("unknown"),
        }),
        "dropped" => Ok(TxEvent::Dropped {
            error: extract_json_str(json, "\"error\":\"").unwrap_or(""),
        }),
        "error" => Ok(TxEvent::Error {
            error: extract_json_str(json, "\"error\":\"").unwrap_or("unknown"),
        }),
        other => Err(crate::Error::Decode(format!("unknown tx event: {other}"))),
    }
}

fn parse_tx_block(json: &str) -> Option<TxBlock> {
    let block = extract_json_object(json, "\"block\":")?;
    if block == "null" {
        return None;
    }
    let hash = extract_json_str(&block, "\"hash\":\"")?.to_string();
    let index = extract_json_object(&block, "\"index\":")?.parse().ok()?;
    Some(TxBlock { hash, index })
}

impl<R: Rpc + RpcSubscription> ChainHead<R> {
    /// Create a new ChainHead session. Fetches genesis hash and starts follow subscription.
    pub async fn new(mut rpc: R) -> crate::Result<Self> {
        let result = rpc
            .rpc("chainSpec_v1_genesisHash", "[]")
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;
        let genesis_hex = result_as_str(&result)
            .map(Into::into)
            .ok_or_else(|| crate::Error::Decode("genesis hash not a string".into()))?;

        let mut genesis_hash = [0u8; 32];
        hex::decode_to_slice(
            <String as AsRef<str>>::as_ref(&genesis_hex).trim_start_matches("0x"),
            &mut genesis_hash,
        )
        .map_err(|_| crate::Error::Decode("genesis hash hex decode failed".into()))?;

        let follow_sub_id = rpc
            .subscribe("chainHead_v1_follow", "[true]")
            .await
            .map_err(|e| crate::Error::Node(format!("follow subscribe failed: {e}")))?;

        let mut ch = ChainHead {
            rpc,
            follow_sub_id,
            finalized_hash: String::new(),
            genesis_hash,
            storage_accum: BTreeMap::new(),
            pending_unpin: Vec::new(),
            needs_refollow: false,
            retained_hashes: BTreeMap::new(),
            event_queue: VecDeque::new(),
            active_operation: None,
            active_tx_watch: None,
        };

        ch.wait_initialized().await?;

        Ok(ch)
    }

    /// Poll the subscription until the initialized event arrives.
    async fn wait_initialized(&mut self) -> crate::Result<()> {
        loop {
            let (_, event_json) = self.next_follow_event().await?;
            let event = parse_follow_event(&event_json)
                .map_err(|e| crate::Error::Decode(format!("follow event: {e}")))?;

            if let FollowEvent::Initialized {
                finalized_block_hashes,
                ..
            } = event
            {
                if let Some(h) = finalized_block_hashes.last() {
                    self.finalized_hash = h.to_string();
                }
                // Queue unpin for all initialized blocks except the latest finalized
                for h in finalized_block_hashes.iter().rev().skip(1) {
                    self.pending_unpin.push(h.to_string());
                }
                return Ok(());
            }
        }
    }

    /// Wait for the next event on the follow subscription, buffering events
    /// from other subscriptions for later retrieval.
    async fn next_follow_event(&mut self) -> crate::Result<(String, String)> {
        loop {
            let (sub_id, value) = self.rpc.next_event().await.ok_or(crate::Error::Node(
                "subscription closed before receiving event".into(),
            ))?;
            if sub_id == self.follow_sub_id {
                return Ok((sub_id, value));
            }
            // Buffer events from other subscriptions
            self.rpc_rebuffer(sub_id, value);
        }
    }

    /// Put an event back into the transport's buffer for later retrieval.
    fn rpc_rebuffer(&mut self, _sub_id: String, _value: String) {
        // Events from non-follow subscriptions are currently discarded.
        // Archive operations use wait_for_archive_event which handles routing.
        log::trace!("discarding event from non-follow subscription");
    }

    /// Fetch and decode the block header at a pinned block hash.
    pub async fn header(&mut self, block_hash: &str) -> crate::Result<BlockHeader> {
        let result = self
            .rpc
            .rpc(
                "chainHead_v1_header",
                &format!(r#"["{}","{}"]"#, self.follow_sub_id, block_hash),
            )
            .await
            .map_err(|e| crate::Error::Node(format!("header: {e}")))?;

        let Some(hex) = result_as_str(&result) else {
            // `null` means the followed block is no longer pinned. This most
            // commonly happens when a cold light client emits enough blocks
            // during a large metadata decode for the node to stop the follow
            // subscription. Mark it for re-follow so callers can recover.
            if result.trim() == "null" {
                self.needs_refollow = true;
                return Err(crate::Error::SubscriptionClosed);
            }
            return Err(crate::Error::Decode("header response not a string".into()));
        };

        decode_header(hex)
    }

    /// Wait for the next user-visible chain event.
    ///
    /// Drains buffered events first, then reads from the follow subscription.
    /// Internal operation events are handled transparently.
    ///
    /// `NewBlock` events have `number: 0` — call [`header()`](Self::header)
    /// with the hash to get the block number if needed.
    pub async fn next_chain_event(&mut self) -> crate::Result<ChainEvent> {
        if let Some(event) = self.event_queue.pop_front() {
            return Ok(event);
        }
        loop {
            let (sub_id, event_json) = self
                .rpc
                .next_event()
                .await
                .ok_or(crate::Error::SubscriptionClosed)?;

            if sub_id != self.follow_sub_id {
                continue;
            }

            let event = parse_follow_event(&event_json)
                .map_err(|e| crate::Error::Decode(format!("follow event: {e}")))?;

            match event {
                FollowEvent::Stop => {
                    self.needs_refollow = true;
                    return Err(crate::Error::SubscriptionClosed);
                }
                // Skip internal operation events
                FollowEvent::OperationStorageItems { .. }
                | FollowEvent::OperationStorageDone { .. }
                | FollowEvent::OperationCallDone { .. }
                | FollowEvent::OperationError { .. }
                | FollowEvent::OperationInaccessible { .. }
                | FollowEvent::OperationWaitingForContinue { .. } => continue,
                other => {
                    self.record_lifecycle_event(other);
                    if let Some(event) = self.event_queue.pop_front() {
                        return Ok(event);
                    }
                }
            }
        }
    }

    /// Return a buffered chain event without blocking.
    pub fn try_next_chain_event(&mut self) -> Option<ChainEvent> {
        self.event_queue.pop_front()
    }

    /// Flush all pending unpins in a single batch RPC call.
    async fn flush_unpins(&mut self) {
        let hashes = core::mem::take(&mut self.pending_unpin)
            .into_iter()
            .filter(|hash| !self.retained_hashes.contains_key(hash))
            .collect::<Vec<_>>();
        if hashes.is_empty() {
            return;
        }
        log::debug!("unpinning {} blocks", hashes.len());
        // Build the hash array as a JSON string
        let mut hash_arr = String::from("[");
        for (i, h) in hashes.iter().enumerate() {
            if i > 0 {
                hash_arr.push(',');
            }
            hash_arr.push('"');
            hash_arr.push_str(h);
            hash_arr.push('"');
        }
        hash_arr.push(']');
        let _ = self
            .rpc
            .rpc(
                "chainHead_v1_unpin",
                &format!(r#"["{}",{}]"#, self.follow_sub_id, hash_arr),
            )
            .await;
    }

    /// Keep a currently pinned block available across paged operations.
    pub fn retain_block(&mut self, block_hash: &str) {
        *self
            .retained_hashes
            .entry(block_hash.to_string())
            .or_insert(0) += 1;
        self.pending_unpin.retain(|hash| hash != block_hash);
    }

    /// Release one snapshot lease. A non-current block is unpinned lazily on
    /// the next operation.
    pub fn release_block(&mut self, block_hash: &str) {
        let Some(count) = self.retained_hashes.get_mut(block_hash) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            self.retained_hashes.remove(block_hash);
            if self.finalized_hash != block_hash {
                self.pending_unpin.push(block_hash.to_string());
            }
        }
    }

    /// Record a lifecycle event. Updates internal state, queues unpins,
    /// and buffers user-visible chain events.
    fn record_lifecycle_event(&mut self, event: FollowEvent<'_>) {
        match event {
            FollowEvent::Initialized {
                finalized_block_hashes,
                ..
            } => {
                if let Some(h) = finalized_block_hashes.last() {
                    self.finalized_hash = h.to_string();
                }
            }
            FollowEvent::NewBlock {
                block_hash,
                parent_block_hash,
                has_new_runtime,
            } => {
                self.event_queue.push_back(ChainEvent::NewBlock {
                    hash: block_hash.into(),
                    parent: parent_block_hash.into(),
                    number: 0, // resolved in next_chain_event via header RPC
                    is_new_runtime: has_new_runtime,
                });
                // Don't unpin new blocks — keep them queryable until finalized/pruned
            }
            FollowEvent::Finalized {
                finalized_block_hashes,
                pruned_block_hashes,
            } => {
                let old = core::mem::take(&mut self.finalized_hash);

                if !old.is_empty() {
                    self.pending_unpin.push(old);
                }
                for h in &pruned_block_hashes {
                    self.pending_unpin.push(h.to_string());
                }
                for h in finalized_block_hashes.iter().rev().skip(1) {
                    self.pending_unpin.push(h.to_string());
                }

                if let Some(new) = finalized_block_hashes.last() {
                    self.finalized_hash = new.to_string();
                    // Remove the new finalized hash from unpin queue — we need it pinned
                    self.pending_unpin.retain(|h| h != *new);
                }

                self.event_queue.push_back(ChainEvent::Finalized {
                    hashes: finalized_block_hashes
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                    pruned: pruned_block_hashes.iter().map(|s| s.to_string()).collect(),
                });
            }
            FollowEvent::BestBlockChanged { best_block_hash } => {
                self.event_queue.push_back(ChainEvent::BestBlock {
                    hash: best_block_hash.into(),
                });
            }
            _ => {}
        }
    }

    /// Drain queued events, flush all pending unpins, ensure we have a pinned
    /// finalized block. If the subscription was stopped, re-subscribe.
    async fn prepare_operation(&mut self) -> crate::Result<String> {
        let mut stopped = false;

        while let Some((sub_id, event_json)) = self.rpc.try_next_event() {
            if sub_id != self.follow_sub_id {
                continue;
            }
            let event = match parse_follow_event(&event_json) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if matches!(event, FollowEvent::Stop) {
                stopped = true;
                break;
            }
            self.record_lifecycle_event(event);
        }

        self.flush_unpins().await;

        if stopped || self.needs_refollow {
            self.needs_refollow = false;
            // A stopped follow invalidates every server-side pin. Callers
            // holding leases will receive `SubscriptionClosed`/inaccessible
            // on their next operation and must restart the snapshot.
            self.retained_hashes.clear();
            // Unsubscribe old follow before creating a new one
            let _ = self
                .rpc
                .unsubscribe("chainHead_v1_unfollow", &self.follow_sub_id)
                .await;
            self.follow_sub_id = self
                .rpc
                .subscribe("chainHead_v1_follow", "[true]")
                .await
                .map_err(|e| crate::Error::Node(format!("refollow failed: {e}")))?;
            self.finalized_hash.clear();
            self.pending_unpin.clear();
            self.wait_initialized().await?;
            self.flush_unpins().await;
        }

        Ok(self.finalized_hash.clone())
    }

    async fn stop_operation(&mut self, operation_id: &str) -> crate::Result<()> {
        let params = format!(r#"["{}","{}"]"#, self.follow_sub_id, operation_id);
        let stop = self.rpc.rpc("chainHead_v1_stopOperation", &params);
        #[cfg(feature = "std")]
        {
            crate::time::timeout(core::time::Duration::from_secs(5), stop)
                .await
                .map_err(|_| crate::Error::ConnectionTimeout)?
                .map(|_| ())
                .map_err(|error| crate::Error::Node(format!("stop operation: {error}")))
        }
        #[cfg(not(feature = "std"))]
        stop.await
            .map(|_| ())
            .map_err(|error| crate::Error::Node(format!("stop operation: {error}")))
    }

    /// Stop the operation left behind when a higher-level future was dropped.
    pub async fn cancel_active_operation(&mut self) -> crate::Result<()> {
        let mut first_error = None;
        if let Some(operation_id) = self.active_operation.take() {
            self.storage_accum.remove(&operation_id);
            if let Err(error) = self.stop_operation(&operation_id).await {
                first_error = Some(error);
            }
        }
        if let Some(subscription_id) = self.active_tx_watch.clone()
            && let Err(error) = self.unwatch_transaction(&subscription_id).await
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Poll the subscription until we get a result for the given operation,
    /// processing other follow events as side effects.
    async fn wait_for_operation(&mut self, target: &str) -> crate::Result<OperationResult> {
        self.active_operation = Some(target.to_string());
        let result = async {
            loop {
                let (sub_id, event_json) = self
                    .rpc
                    .next_event()
                    .await
                    .ok_or(crate::Error::SubscriptionClosed)?;

                // Skip events from non-follow subscriptions
                if sub_id != self.follow_sub_id {
                    continue;
                }

                let event = parse_follow_event(&event_json)
                    .map_err(|e| crate::Error::Decode(format!("follow event: {e}")))?;

                match event {
                    FollowEvent::Stop => {
                        return Err(crate::Error::SubscriptionClosed);
                    }
                    FollowEvent::OperationStorageItems {
                        operation_id,
                        items,
                    } => {
                        let entry = self
                            .storage_accum
                            .entry(operation_id.to_string())
                            .or_default();
                        for item in items {
                            entry.push(item);
                        }
                    }
                    FollowEvent::OperationStorageDone { operation_id } => {
                        let items = self.storage_accum.remove(operation_id).unwrap_or_default();
                        if operation_id == target {
                            return Ok(OperationResult::StorageItems(items));
                        }
                    }
                    FollowEvent::OperationCallDone {
                        operation_id,
                        output,
                    } => {
                        if operation_id == target {
                            return Ok(OperationResult::CallDone(output.into()));
                        }
                    }
                    FollowEvent::OperationError {
                        operation_id,
                        error,
                    } => {
                        if operation_id == target {
                            return Ok(OperationResult::Error(error.into()));
                        }
                    }
                    FollowEvent::OperationInaccessible { operation_id } => {
                        if operation_id == target {
                            return Ok(OperationResult::Error("block inaccessible".into()));
                        }
                    }
                    FollowEvent::OperationWaitingForContinue { operation_id } => {
                        if operation_id == target {
                            let _ = self
                                .rpc
                                .rpc(
                                    "chainHead_v1_continue",
                                    &format!(r#"["{}","{}"]"#, self.follow_sub_id, operation_id),
                                )
                                .await;
                        }
                    }
                    other => self.record_lifecycle_event(other),
                }
            }
        }
        .await;
        if result.is_err() {
            let _ = self.stop_operation(target).await;
            self.storage_accum.remove(target);
        }
        if self.active_operation.as_deref() == Some(target) {
            self.active_operation = None;
        }
        result
    }

    /// Send a storage query at a specific pinned block hash and wait for results.
    async fn storage_at_hash(
        &mut self,
        hash: &str,
        keys: &[String],
    ) -> crate::Result<Vec<StorageItem>> {
        // Drain buffered events and flush unpins before querying
        self.prepare_operation().await?;
        self.storage_with_hash(hash, keys).await
    }

    /// Send a storage query and wait for results.
    async fn storage(&mut self, keys: &[String]) -> crate::Result<Vec<StorageItem>> {
        let hash = self.prepare_operation().await?;
        self.storage_with_hash(&hash, keys).await
    }

    /// Query storage at a given block hash (must be pinned).
    async fn storage_with_hash(
        &mut self,
        hash: &str,
        keys: &[String],
    ) -> crate::Result<Vec<StorageItem>> {
        let mut items = Vec::with_capacity(keys.len());
        for key in keys {
            items.extend(self.storage_one_with_hash(hash, key).await?);
        }
        Ok(items)
    }

    async fn storage_one_with_hash(
        &mut self,
        hash: &str,
        key: &str,
    ) -> crate::Result<Vec<StorageItem>> {
        let items_json = format!(r#"[{{"key":"{key}","type":"value"}}]"#);

        let result = self
            .rpc
            .rpc(
                "chainHead_v1_storage",
                &format!(
                    r#"["{}","{}",{},null]"#,
                    self.follow_sub_id, hash, items_json
                ),
            )
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        let started = parse_operation_started(&result)
            .map_err(|e| crate::Error::Node(format!("bad storage response: {e}")))?;

        match started {
            OperationStarted::Started {
                operation_id,
                discarded_items: 0,
            } => match self.wait_for_operation(operation_id).await? {
                OperationResult::StorageItems(items) => Ok(items),
                OperationResult::Error(e) => Err(crate::Error::Node(e)),
                _ => Err(crate::Error::Node("unexpected result".into())),
            },
            OperationStarted::Started {
                operation_id,
                discarded_items,
            } => {
                let _ = self.stop_operation(operation_id).await;
                Err(crate::Error::OperationFailed(format!(
                    "chainHead discarded {discarded_items} storage item(s)"
                )))
            }
            OperationStarted::LimitReached => Err(crate::Error::Node(
                "chainHead operation limit reached".into(),
            )),
        }
    }

    /// Legacy state RPC used for genuinely bounded, cursor-based key scans.
    /// Values are still read through chainHead/archive at the same explicit
    /// finalized block; this call only discovers the raw keys.
    async fn keys_paged_at_hash(
        &mut self,
        prefix: &[u8],
        size: u16,
        start_key: Option<&[u8]>,
        block_hash: &[u8; 32],
    ) -> crate::Result<Vec<crate::RawKey>> {
        let prefix = to_hex(prefix);
        let start = start_key
            .map(|key| format!(r#""{}""#, to_hex(key)))
            .unwrap_or_else(|| "null".into());
        let hash = to_hex(block_hash);
        let raw = self
            .rpc
            .rpc(
                "state_getKeysPaged",
                &format!(r#"["{prefix}",{size},{start},"{hash}"]"#),
            )
            .await
            .map_err(|error| crate::Error::Node(format!("paged keys: {error}")))?;

        parse_string_values(&raw)
            .into_iter()
            .map(|key| {
                hex::decode(key.trim_start_matches("0x"))
                    .map_err(|_| crate::Error::Decode("paged key hex decode failed".into()))
            })
            .collect()
    }

    /// Execute one single-item storage operation. Smoldot-light 1.3 accounts
    /// operation slots per item but releases them per operation, so combining
    /// items here would permanently consume slots. Partial unordered results
    /// are never exposed as a page: an oversized proof requests a split.
    async fn key_item_operation_at_hash(
        &mut self,
        hash: &str,
        key_prefix: &[u8],
        item_type: &str,
        after_key: Option<&[u8]>,
        collect_limit: usize,
    ) -> crate::Result<KeyItemOperation> {
        let key = to_hex(key_prefix);
        let items_json = format!(r#"[{{"key":"{key}","type":"{item_type}"}}]"#);
        let result = self
            .rpc
            .rpc(
                "chainHead_v1_storage",
                &format!(
                    r#"["{}","{}",{},null]"#,
                    self.follow_sub_id, hash, items_json
                ),
            )
            .await
            .map_err(|error| crate::Error::Node(error.to_string()))?;
        let operation_id = match parse_operation_started(&result)
            .map_err(|error| crate::Error::Node(format!("bad storage response: {error}")))?
        {
            OperationStarted::Started {
                operation_id,
                discarded_items: 0,
            } => operation_id.to_string(),
            OperationStarted::Started {
                operation_id,
                discarded_items,
            } => {
                let operation_id = operation_id.to_string();
                let _ = self.stop_operation(&operation_id).await;
                return Err(crate::Error::OperationFailed(format!(
                    "chainHead discarded {discarded_items} key item(s)"
                )));
            }
            OperationStarted::LimitReached => {
                return Err(crate::Error::Node(
                    "chainHead operation limit reached".into(),
                ));
            }
        };

        self.active_operation = Some(operation_id.clone());
        let outcome = async {
            let mut keys = Vec::with_capacity(collect_limit.saturating_add(1).min(64));
            loop {
                let (sub_id, event_json) = self
                    .rpc
                    .next_event()
                    .await
                    .ok_or(crate::Error::SubscriptionClosed)?;
                if sub_id != self.follow_sub_id {
                    continue;
                }
                let event = parse_follow_event(&event_json)
                    .map_err(|error| crate::Error::Decode(format!("follow event: {error}")))?;
                match event {
                    FollowEvent::OperationStorageItems {
                        operation_id: event_id,
                        items,
                    } if event_id == operation_id => {
                        for item in items {
                            if item.hash.is_none() {
                                continue;
                            }
                            let decoded =
                                hex::decode(item.key.trim_start_matches("0x")).map_err(|_| {
                                    crate::Error::Decode("paged key hex decode failed".into())
                                })?;
                            if after_key.is_some_and(|after| decoded.as_slice() <= after) {
                                continue;
                            }
                            keys.push(decoded);
                            if keys.len() > collect_limit {
                                let _ = self.stop_operation(&operation_id).await;
                                return Ok(KeyItemOperation::TooLarge);
                            }
                        }
                    }
                    FollowEvent::OperationStorageDone {
                        operation_id: event_id,
                    } if event_id == operation_id => {
                        keys.sort_unstable();
                        keys.dedup();
                        return Ok(KeyItemOperation::Complete(keys));
                    }
                    FollowEvent::OperationWaitingForContinue {
                        operation_id: event_id,
                    } if event_id == operation_id => {
                        let _ = self
                            .rpc
                            .rpc(
                                "chainHead_v1_continue",
                                &format!(r#"["{}","{}"]"#, self.follow_sub_id, operation_id),
                            )
                            .await;
                    }
                    FollowEvent::OperationError {
                        operation_id: event_id,
                        error,
                    } if event_id == operation_id => {
                        return Err(crate::Error::Node(error.into()));
                    }
                    FollowEvent::OperationInaccessible {
                        operation_id: event_id,
                    } if event_id == operation_id => return Ok(KeyItemOperation::Inaccessible),
                    FollowEvent::Stop => {
                        self.needs_refollow = true;
                        return Err(crate::Error::SubscriptionClosed);
                    }
                    other => self.record_lifecycle_event(other),
                }
            }
        }
        .await;
        if outcome.is_err() {
            let _ = self.stop_operation(&operation_id).await;
        }
        if self.active_operation.as_deref() == Some(operation_id.as_str()) {
            self.active_operation = None;
        }
        outcome
    }

    /// Collect the exact key and its descendants through separate one-item
    /// operations. Only a completed, sorted descendant proof can produce a
    /// cursor. Oversized or inaccessible descendant proofs split adaptively.
    async fn key_operation_at_hash(
        &mut self,
        hash: &str,
        key_prefix: &[u8],
        after_key: Option<&[u8]>,
        collect_limit: usize,
    ) -> crate::Result<KeyOperation> {
        let exact_keys = if after_key.is_some_and(|after| key_prefix <= after) {
            Vec::new()
        } else {
            match self
                .key_item_operation_at_hash(hash, key_prefix, "hash", after_key, 1)
                .await?
            {
                KeyItemOperation::Complete(keys) => keys,
                KeyItemOperation::TooLarge | KeyItemOperation::Inaccessible => {
                    return Err(crate::Error::OperationFailed(
                        "exact trie key is inaccessible".into(),
                    ));
                }
            }
        };

        let descendant_limit = collect_limit.saturating_sub(exact_keys.len());
        match self
            .key_item_operation_at_hash(
                hash,
                key_prefix,
                "descendantsHashes",
                after_key,
                descendant_limit,
            )
            .await?
        {
            KeyItemOperation::Complete(mut keys) => {
                keys.extend(exact_keys);
                keys.sort_unstable();
                keys.dedup();
                Ok(KeyOperation::Complete(keys))
            }
            KeyItemOperation::TooLarge | KeyItemOperation::Inaccessible => {
                Ok(KeyOperation::Split { exact_keys })
            }
        }
    }

    /// Traverse lexicographic state-trie partitions instead of asking a light
    /// peer for an unbounded proof of every descendant below a map prefix.
    /// The opaque cursor records the current partition and optional last key.
    async fn partitioned_keys_page_at_hash(
        &mut self,
        prefix: &[u8],
        limit: u16,
        cursor: Option<crate::RawKey>,
        block_hash: &[u8; 32],
    ) -> crate::Result<crate::RawKeysPage> {
        if limit == 0 {
            return Err(crate::Error::BadInput);
        }
        let hash = to_hex(block_hash);
        if !self.retained_hashes.contains_key(&hash) {
            return Err(crate::Error::OperationFailed(
                "block snapshot is not retained".into(),
            ));
        }
        let (mut partition, mut after_key) = decode_light_page_cursor(prefix, cursor)?;

        self.prepare_operation().await?;
        if !self.retained_hashes.contains_key(&hash) {
            return Err(crate::Error::SubscriptionClosed);
        }

        let mut page_keys = Vec::with_capacity(usize::from(limit));
        let mut operations = 0usize;
        loop {
            if operations >= LIGHT_PAGE_MAX_OPERATIONS {
                let next_cursor = encode_light_page_cursor(&partition, after_key.as_deref())?;
                return Ok(crate::RawKeysPage {
                    keys: page_keys,
                    next_cursor: Some(next_cursor),
                });
            }

            let mut key_prefix = Vec::with_capacity(prefix.len() + partition.len());
            key_prefix.extend_from_slice(prefix);
            key_prefix.extend_from_slice(&partition);
            let remaining = usize::from(limit) - page_keys.len();
            let operation = self
                .key_operation_at_hash(&hash, &key_prefix, after_key.as_deref(), remaining + 1)
                .await?;
            operations += 1;
            self.flush_unpins().await;

            match operation {
                KeyOperation::Complete(mut keys) => {
                    if keys.len() > remaining {
                        keys.truncate(remaining);
                        page_keys.extend(keys);
                        let last = page_keys.last().ok_or(crate::Error::BadInput)?;
                        let next_cursor = encode_light_page_cursor(&partition, Some(last))?;
                        return Ok(crate::RawKeysPage {
                            keys: page_keys,
                            next_cursor: Some(next_cursor),
                        });
                    }
                    page_keys.append(&mut keys);

                    let next_exists = advance_light_page_partition(&mut partition);
                    after_key = None;

                    if page_keys.len() == usize::from(limit) {
                        return Ok(crate::RawKeysPage {
                            keys: page_keys,
                            next_cursor: next_exists
                                .then(|| encode_light_page_cursor(&partition, None))
                                .transpose()?,
                        });
                    }
                    if !next_exists {
                        return Ok(crate::RawKeysPage {
                            keys: page_keys,
                            next_cursor: None,
                        });
                    }
                }
                KeyOperation::Split { mut exact_keys } => {
                    if partition.len() >= LIGHT_PAGE_MAX_DEPTH {
                        return Err(crate::Error::OperationFailed(
                            "light-client trie partition remains too large".into(),
                        ));
                    }

                    exact_keys.truncate(remaining);
                    page_keys.append(&mut exact_keys);
                    if page_keys.len() == usize::from(limit) {
                        let last = page_keys.last().cloned().ok_or(crate::Error::BadInput)?;
                        let next_cursor = encode_light_page_cursor(&partition, Some(&last))?;
                        return Ok(crate::RawKeysPage {
                            keys: page_keys,
                            next_cursor: Some(next_cursor),
                        });
                    }

                    let next_byte = after_key
                        .as_deref()
                        .and_then(|key| key.get(prefix.len() + partition.len()))
                        .copied()
                        .unwrap_or(0);
                    partition.push(next_byte);
                    let mut expected_prefix = Vec::with_capacity(prefix.len() + partition.len());
                    expected_prefix.extend_from_slice(prefix);
                    expected_prefix.extend_from_slice(&partition);
                    if after_key
                        .as_deref()
                        .is_some_and(|key| !key.starts_with(&expected_prefix))
                    {
                        after_key = None;
                    }
                }
            }
        }
    }

    /// Read storage through the legacy proof-backed method at an explicit
    /// hash. Smoldot can verify these requests without an archive height
    /// lookup and without keeping the block pinned by `chainHead`.
    async fn legacy_storage_items_at_hash(
        &mut self,
        keys: Vec<crate::RawKey>,
        block_hash: &[u8; 32],
    ) -> crate::Result<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        let hash = to_hex(block_hash);
        let mut values = Vec::with_capacity(keys.len());
        for key in keys {
            let raw = self
                .rpc
                .rpc(
                    "state_getStorage",
                    &format!(r#"["{}","{}"]"#, to_hex(&key), hash),
                )
                .await
                .map_err(|error| crate::Error::Node(format!("storage at hash: {error}")))?;
            let value =
                if raw.trim() == "null" {
                    None
                } else {
                    let encoded = result_as_str(&raw).ok_or_else(|| {
                        crate::Error::Decode("storage response is neither hex nor null".into())
                    })?;
                    Some(hex::decode(encoded.trim_start_matches("0x")).map_err(|_| {
                        crate::Error::Decode("storage value hex decode failed".into())
                    })?)
                };
            values.push((key, value));
        }
        Ok(values)
    }

    /// Fetch metadata through the legacy proof-backed endpoint. Unlike a
    /// chainHead call, this remains usable for an authenticated finalized hash
    /// after its follow pin has moved on.
    async fn legacy_metadata_at_hash(&mut self, block_hash: &[u8; 32]) -> crate::Result<Metadata> {
        let raw = self
            .rpc
            .rpc(
                "state_getMetadata",
                &format!(r#"["{}"]"#, to_hex(block_hash)),
            )
            .await
            .map_err(|error| crate::Error::Node(format!("metadata at hash: {error}")))?;
        let encoded = result_as_str(&raw)
            .ok_or_else(|| crate::Error::Decode("metadata response is not hex".into()))?;
        let bytes = hex::decode(encoded.trim_start_matches("0x"))
            .map_err(|_| crate::Error::Decode("metadata hex decode failed".into()))?;
        meta::from_bytes(&bytes)
    }

    /// Resolve a header through the legacy verified-header endpoint so callers
    /// cannot attach an arbitrary number to a valid block hash.
    async fn legacy_block_info_at_hash(
        &mut self,
        block_hash: &[u8; 32],
    ) -> crate::Result<meta::BlockInfo> {
        let raw = self
            .rpc
            .rpc("chain_getHeader", &format!(r#"["{}"]"#, to_hex(block_hash)))
            .await
            .map_err(|error| crate::Error::Node(format!("header at hash: {error}")))?;
        if raw.trim() == "null" {
            return Err(crate::Error::BadBlockNumber);
        }
        let number = extract_json_str(&raw, "\"number\":\"")
            .and_then(|number| u64::from_str_radix(number.trim_start_matches("0x"), 16).ok())
            .ok_or_else(|| crate::Error::Decode("header number is missing or invalid".into()))?;
        let parent_hex = extract_json_str(&raw, "\"parentHash\":\"")
            .ok_or_else(|| crate::Error::Decode("header parentHash is missing".into()))?;
        let mut parent = [0u8; 32];
        hex::decode_to_slice(parent_hex.trim_start_matches("0x"), &mut parent)
            .map_err(|_| crate::Error::Decode("header parent hash is invalid".into()))?;
        Ok(meta::BlockInfo {
            number,
            hash: *block_hash,
            parent,
        })
    }

    /// Execute a runtime call at a specific pinned block hash.
    pub async fn runtime_call_at(
        &mut self,
        block_hash: &str,
        function: &str,
        call_data: &str,
    ) -> crate::Result<Vec<u8>> {
        self.prepare_operation().await?;
        self.runtime_call_with_hash(block_hash, function, call_data)
            .await
    }

    /// Execute a runtime call at the current finalized block.
    async fn runtime_call(&mut self, function: &str, call_data: &str) -> crate::Result<Vec<u8>> {
        let hash = self.prepare_operation().await?;
        self.runtime_call_with_hash(&hash, function, call_data)
            .await
    }

    async fn runtime_call_with_hash(
        &mut self,
        hash: &str,
        function: &str,
        call_data: &str,
    ) -> crate::Result<Vec<u8>> {
        let result = self
            .rpc
            .rpc(
                "chainHead_v1_call",
                &format!(
                    r#"["{}","{}","{}","{}"]"#,
                    self.follow_sub_id, hash, function, call_data
                ),
            )
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        let started = parse_operation_started(&result)
            .map_err(|e| crate::Error::Node(format!("bad call response: {e}")))?;

        match started {
            OperationStarted::Started {
                operation_id,
                discarded_items: 0,
            } => match self.wait_for_operation(operation_id).await? {
                OperationResult::CallDone(hex_output) => {
                    hex::decode(hex_output.trim_start_matches("0x"))
                        .map_err(|_| crate::Error::Decode("runtime call hex decode".into()))
                }
                OperationResult::Error(e) => Err(crate::Error::Node(e)),
                _ => Err(crate::Error::Node("unexpected result".into())),
            },
            OperationStarted::Started {
                operation_id,
                discarded_items,
            } => {
                let _ = self.stop_operation(operation_id).await;
                Err(crate::Error::OperationFailed(format!(
                    "chainHead discarded {discarded_items} call item(s)"
                )))
            }
            OperationStarted::LimitReached => Err(crate::Error::Node(
                "chainHead operation limit reached".into(),
            )),
        }
    }

    // --- Archive API methods ---

    /// Resolve a block number to a block hash via `archive_v1_hashByHeight`.
    async fn archive_hash_by_height(&mut self, height: u64) -> crate::Result<String> {
        let result = self
            .rpc
            .rpc("archive_v1_hashByHeight", &format!("[{}]", height))
            .await
            .map_err(|e| crate::Error::Node(format!("archive_v1_hashByHeight: {e}")))?;

        // Returns an array of hashes (usually one for canonical chain).
        // The result is the raw JSON array, e.g. `["0xabc..."]`.
        // Wrap it so extract_str_array can find the array via a marker.
        let wrapped = format!(r#"{{"v":{}}}"#, result.trim());
        let hashes = extract_str_array(&wrapped, "\"v\":");

        hashes
            .into_iter()
            .next()
            .map(|s| s.to_string())
            .ok_or(crate::Error::BadBlockNumber)
    }

    /// Query storage at a historical block via `archive_v1_storage` (subscription-based).
    async fn archive_storage(
        &mut self,
        block_hash: &str,
        keys: &[String],
    ) -> crate::Result<Vec<StorageItem>> {
        let mut items_json = String::from("[");
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                items_json.push(',');
            }
            items_json.push_str(&format!(r#"{{"key":"{}","type":"value"}}"#, k));
        }
        items_json.push(']');

        let archive_sub_id = self
            .rpc
            .subscribe(
                "archive_v1_storage",
                &format!(r#"["{}",{}]"#, block_hash, items_json),
            )
            .await
            .map_err(|e| crate::Error::Node(format!("archive_v1_storage: {e}")))?;

        self.wait_for_archive_storage(&archive_sub_id).await
    }

    /// Wait for archive storage events, routing follow events to lifecycle handling.
    async fn wait_for_archive_storage(
        &mut self,
        archive_sub_id: &str,
    ) -> crate::Result<Vec<StorageItem>> {
        let mut result_items = Vec::new();

        loop {
            let (sub_id, event_json) = self
                .rpc
                .next_event()
                .await
                .ok_or(crate::Error::SubscriptionClosed)?;

            if sub_id == archive_sub_id {
                let event = parse_archive_storage_event(&event_json)
                    .map_err(|e| crate::Error::Decode(format!("archive event: {e}")))?;

                match event {
                    ArchiveStorageEvent::Items { items } => {
                        for item in items {
                            result_items.push(item);
                        }
                    }
                    ArchiveStorageEvent::Done => return Ok(result_items),
                    ArchiveStorageEvent::Error { error } => {
                        return Err(crate::Error::Node(format!("archive storage: {error}")));
                    }
                    ArchiveStorageEvent::WaitingForContinue => {
                        let _ = self
                            .rpc
                            .rpc(
                                "archive_v1_storageContinue",
                                &format!(r#"["{}"]"#, archive_sub_id),
                            )
                            .await;
                    }
                }
            } else if sub_id == self.follow_sub_id {
                // Process follow events that arrive while waiting for archive results
                if let Ok(event) = parse_follow_event(&event_json) {
                    match event {
                        FollowEvent::Stop => {
                            self.needs_refollow = true;
                        }
                        other => self.record_lifecycle_event(other),
                    }
                }
            }
            // Events from unknown subscriptions are discarded
        }
    }
}

// --- Public query-at-hash ---

/// Decode a SCALE-encoded block header from hex.
fn decode_header(hex: &str) -> crate::Result<BlockHeader> {
    let bytes = hex::decode(hex.trim_start_matches("0x"))
        .map_err(|_| crate::Error::Decode("header hex".into()))?;
    let mut cursor = &bytes[..];

    // parent_hash: 32 bytes
    if cursor.len() < 32 {
        return Err(crate::Error::Decode(
            "header too short for parent_hash".into(),
        ));
    }
    let parent_hash = format!("0x{}", hex::encode(&cursor[..32]));
    cursor = &cursor[32..];

    // number: Compact<u64>
    let number = <codec::Compact<u64>>::decode(&mut cursor)
        .map_err(|_| crate::Error::Decode("header block number".into()))?
        .0;

    // state_root: 32 bytes
    if cursor.len() < 32 {
        return Err(crate::Error::Decode(
            "header too short for state_root".into(),
        ));
    }
    let state_root = format!("0x{}", hex::encode(&cursor[..32]));
    cursor = &cursor[32..];

    // extrinsics_root: 32 bytes
    if cursor.len() < 32 {
        return Err(crate::Error::Decode(
            "header too short for extrinsics_root".into(),
        ));
    }
    let extrinsics_root = format!("0x{}", hex::encode(&cursor[..32]));

    Ok(BlockHeader {
        parent_hash,
        number,
        state_root,
        extrinsics_root,
    })
}

impl<R: Rpc + RpcSubscription> ChainHead<R> {
    /// Query storage items at a specific block hash (must be a pinned hash
    /// from a recent `NewBlock` event that hasn't been finalized/pruned yet).
    pub async fn get_storage_at_hash(
        &mut self,
        block_hash: &str,
        keys: Vec<crate::RawKey>,
    ) -> crate::Result<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        let hex_keys: Vec<String> = keys.iter().map(|k| to_hex(k)).collect();
        let items = self.storage_at_hash(block_hash, &hex_keys).await?;
        decode_storage_items(&keys, &items)
    }

    async fn unwatch_transaction(&mut self, sub_id: &str) -> crate::Result<()> {
        let unsubscribe = self.rpc.unsubscribe("transactionWatch_v1_unwatch", sub_id);
        #[cfg(feature = "std")]
        let result = crate::time::timeout(core::time::Duration::from_secs(5), unsubscribe)
            .await
            .map_err(|_| crate::Error::ConnectionTimeout)?
            .map_err(|error| crate::Error::Node(format!("tx unwatch: {error}")));
        #[cfg(not(feature = "std"))]
        let result = unsubscribe
            .await
            .map_err(|error| crate::Error::Node(format!("tx unwatch: {error}")));
        if result.is_ok() && self.active_tx_watch.as_deref() == Some(sub_id) {
            self.active_tx_watch = None;
        }
        result
    }

    async fn watch_transaction(
        &mut self,
        ext: &[u8],
        wait_for: crate::WaitFor,
        timeout: Option<core::time::Duration>,
    ) -> crate::Result<crate::TransactionReceipt> {
        let hex = to_hex(ext);
        let sub_id = self
            .rpc
            .subscribe(
                "transactionWatch_v1_submitAndWatch",
                &format!(r#"["{}"]"#, hex),
            )
            .await
            .map_err(|e| crate::Error::Node(format!("tx watch: {e}")))?;
        self.active_tx_watch = Some(sub_id.clone());

        let mut receipt = crate::TransactionReceipt::default();
        #[cfg(feature = "std")]
        let deadline = timeout.and_then(|duration| std::time::Instant::now().checked_add(duration));
        #[cfg(not(feature = "std"))]
        let _ = timeout;

        loop {
            #[cfg(feature = "std")]
            let next_event = if let Some(deadline) = deadline {
                let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now())
                else {
                    let _ = self.unwatch_transaction(&sub_id).await;
                    return Err(crate::Error::ConnectionTimeout);
                };
                match crate::time::timeout(remaining, self.rpc.next_event()).await {
                    Ok(event) => event,
                    Err(_) => {
                        let _ = self.unwatch_transaction(&sub_id).await;
                        return Err(crate::Error::ConnectionTimeout);
                    }
                }
            } else {
                self.rpc.next_event().await
            };
            #[cfg(not(feature = "std"))]
            let next_event = self.rpc.next_event().await;

            let Some((event_sub_id, event_json)) = next_event else {
                let _ = self.unwatch_transaction(&sub_id).await;
                self.active_tx_watch = None;
                return Err(crate::Error::SubscriptionClosed);
            };

            if event_sub_id == sub_id {
                let event = match parse_tx_event(&event_json) {
                    Ok(event) => event,
                    Err(error) => {
                        let _ = self.unwatch_transaction(&sub_id).await;
                        return Err(crate::Error::Decode(format!("tx event: {error}")));
                    }
                };
                match event {
                    TxEvent::BestChainBlockIncluded { block: Some(block) } => {
                        receipt.best_block_hash = Some(block.hash);
                        receipt.extrinsic_index = Some(block.index);
                        if matches!(wait_for, crate::WaitFor::BestBlock) {
                            self.unwatch_transaction(&sub_id).await?;
                            return Ok(receipt);
                        }
                    }
                    TxEvent::BestChainBlockIncluded { block: None } => {
                        // A previous best-chain inclusion was retracted.
                        receipt.best_block_hash = None;
                        receipt.extrinsic_index = None;
                    }
                    TxEvent::Finalized { block } => {
                        receipt.finalized_block_hash = Some(block.hash);
                        receipt.extrinsic_index = Some(block.index);
                        self.active_tx_watch = None;
                        return Ok(receipt);
                    }
                    TxEvent::Invalid { error } => {
                        self.active_tx_watch = None;
                        return Err(crate::Error::OperationFailed(format!(
                            "tx invalid: {error}"
                        )));
                    }
                    TxEvent::Dropped { error } => {
                        self.active_tx_watch = None;
                        return Err(crate::Error::OperationFailed(format!(
                            "tx dropped: {error}"
                        )));
                    }
                    TxEvent::Error { error } => {
                        self.active_tx_watch = None;
                        return Err(crate::Error::OperationFailed(format!("tx error: {error}")));
                    }
                    TxEvent::Validated | TxEvent::Broadcasted => {}
                }
            } else if event_sub_id == self.follow_sub_id
                && let Ok(event) = parse_follow_event(&event_json)
            {
                match event {
                    FollowEvent::Stop => self.needs_refollow = true,
                    other => self.record_lifecycle_event(other),
                }
            }
        }
    }
}

fn decode_storage_items(
    keys: &[crate::RawKey],
    items: &[StorageItem],
) -> crate::Result<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
    let mut result = Vec::new();
    for key in keys {
        let search_key = hex::encode(key);
        let value = items
            .iter()
            .find(|item| item.key.trim_start_matches("0x") == search_key)
            .and_then(|item| {
                item.value
                    .as_ref()
                    .map(|v| hex::decode(v.trim_start_matches("0x")).unwrap_or_default())
            });
        result.push((key.clone(), value));
    }
    Ok(result)
}

// --- Backend implementation ---

impl<R: Rpc + RpcSubscription> crate::Backend for ChainHead<R> {
    async fn get_storage_items(
        &mut self,
        keys: Vec<crate::RawKey>,
        block: Option<u32>,
    ) -> crate::Result<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        let hex_keys: Vec<String> = keys.iter().map(|k| to_hex(k)).collect();

        let items = match block {
            None => self.storage(&hex_keys).await?,
            Some(n) => {
                let hash = self.archive_hash_by_height(n as u64).await?;
                self.archive_storage(&hash, &hex_keys).await?
            }
        };

        decode_storage_items(&keys, &items)
    }

    async fn get_storage_items_at_hash(
        &mut self,
        keys: Vec<crate::RawKey>,
        block_hash: [u8; 32],
    ) -> crate::Result<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        self.legacy_storage_items_at_hash(keys, &block_hash).await
    }

    async fn get_keys_paged(
        &mut self,
        from: crate::RawKey,
        size: u16,
        to: Option<crate::RawKey>,
    ) -> crate::Result<Vec<crate::RawKey>> {
        self.get_keys_paged_at(from, size, to, None).await
    }

    async fn get_keys_paged_at(
        &mut self,
        prefix: crate::RawKey,
        size: u16,
        start_key: Option<crate::RawKey>,
        block: Option<u32>,
    ) -> crate::Result<Vec<crate::RawKey>> {
        let block = self.block_info(block).await?;
        self.keys_paged_at_hash(&prefix, size, start_key.as_deref(), &block.hash)
            .await
    }

    async fn get_keys_paged_at_hash(
        &mut self,
        prefix: crate::RawKey,
        size: u16,
        start_key: Option<crate::RawKey>,
        block_hash: [u8; 32],
    ) -> crate::Result<Vec<crate::RawKey>> {
        self.keys_paged_at_hash(&prefix, size, start_key.as_deref(), &block_hash)
            .await
    }

    async fn get_keys_page_at_hash(
        &mut self,
        prefix: crate::RawKey,
        limit: u16,
        cursor: Option<crate::RawKey>,
        block_hash: [u8; 32],
    ) -> crate::Result<crate::RawKeysPage> {
        self.partitioned_keys_page_at_hash(&prefix, limit, cursor, &block_hash)
            .await
    }

    async fn cancel_active_operation(&mut self) -> crate::Result<()> {
        ChainHead::cancel_active_operation(self).await
    }

    async fn submit(&mut self, ext: &[u8], wait_for_finalization: bool) -> crate::Result<()> {
        self.watch_transaction(
            ext,
            if wait_for_finalization {
                crate::WaitFor::Finalized
            } else {
                crate::WaitFor::BestBlock
            },
            None,
        )
        .await
        .map(|_| ())
    }

    async fn submit_transaction(
        &mut self,
        ext: &crate::EncodedExtrinsic,
        wait_for: crate::WaitFor,
    ) -> crate::Result<crate::TransactionReceipt> {
        self.watch_transaction(&ext.bytes, wait_for, None).await
    }

    async fn submit_transaction_with_timeout(
        &mut self,
        ext: &crate::EncodedExtrinsic,
        wait_for: crate::WaitFor,
        timeout: core::time::Duration,
    ) -> crate::Result<crate::TransactionReceipt> {
        self.watch_transaction(&ext.bytes, wait_for, Some(timeout))
            .await
    }

    async fn inspect_transaction(
        &mut self,
        ext: &crate::EncodedExtrinsic,
    ) -> crate::Result<crate::TransactionReport> {
        let mut report = crate::TransactionReport::default();

        // TransactionPaymentApi::query_info(Extrinsic, u32).
        let mut payment_input = ext.bytes.clone();
        let encoded_len = u32::try_from(ext.bytes.len())
            .map_err(|_| crate::Error::Encode("extrinsic too large".into()))?;
        payment_input.extend_from_slice(&encoded_len.to_le_bytes());
        match self
            .runtime_call("TransactionPaymentApi_query_info", &to_hex(&payment_input))
            .await
        {
            Ok(output) if output.len() >= 33 => {
                let ref_time = u64::from_le_bytes(
                    output[0..8]
                        .try_into()
                        .map_err(|_| crate::Error::Decode("weight ref_time".into()))?,
                );
                let proof_size = u64::from_le_bytes(
                    output[8..16]
                        .try_into()
                        .map_err(|_| crate::Error::Decode("weight proof_size".into()))?,
                );
                let fee_offset = output.len() - 16;
                let partial_fee = u128::from_le_bytes(
                    output[fee_offset..]
                        .try_into()
                        .map_err(|_| crate::Error::Decode("partial fee".into()))?,
                );
                report.weight = Some(crate::TransactionWeight {
                    ref_time,
                    proof_size,
                });
                report.partial_fee = Some(partial_fee);
            }
            Ok(_) => report
                .warnings
                .push("transaction-payment API returned an unknown result shape".into()),
            Err(error) => report
                .warnings
                .push(format!("transaction-payment API unavailable: {error}")),
        }

        // TaggedTransactionQueue::validate_transaction(
        //   TransactionSource::External, Extrinsic, checkpoint_hash
        // ).
        let mut validity_input = vec![2u8];
        validity_input.extend_from_slice(&ext.bytes);
        validity_input.extend_from_slice(&ext.checkpoint_hash);
        match self
            .runtime_call(
                "TaggedTransactionQueue_validate_transaction",
                &to_hex(&validity_input),
            )
            .await
        {
            Ok(output) if output.first() == Some(&0) => {
                report.validity = Some(crate::TransactionValidity::Valid)
            }
            Ok(output) if output.first() == Some(&1) => {
                report.validity = Some(crate::TransactionValidity::Invalid(format!(
                    "0x{}",
                    hex::encode(&output[1..])
                )))
            }
            Ok(_) => {
                report.validity = Some(crate::TransactionValidity::Unknown);
                report
                    .warnings
                    .push("validation API returned an unknown result shape".into());
            }
            Err(error) => {
                report.validity = None;
                report
                    .warnings
                    .push(format!("validation API unavailable: {error}"));
            }
        }

        Ok(report)
    }

    async fn enrich_receipt(
        &mut self,
        mut receipt: crate::TransactionReceipt,
        metadata: &Metadata,
    ) -> crate::Result<crate::TransactionReceipt> {
        let Some(block_hash) = receipt
            .finalized_block_hash
            .as_deref()
            .or(receipt.best_block_hash.as_deref())
        else {
            return Ok(receipt);
        };
        let Some(extrinsic_index) = receipt.extrinsic_index else {
            return Ok(receipt);
        };

        // Transaction-watch blocks are not guaranteed to be pinned by the
        // chainHead subscription, so fetch events through archive storage.
        let key = match crate::resolve_query(metadata, "system/events") {
            Ok(crate::ResolvedQuery::Storage(key)) => key,
            _ => return Ok(receipt),
        };
        let key_bytes = key.key();
        let items = match self
            .archive_storage(block_hash, &[to_hex(&key_bytes)])
            .await
        {
            Ok(items) => items,
            Err(_) => return Ok(receipt),
        };
        let Some(raw) = items
            .iter()
            .find(|item| item.key.trim_start_matches("0x") == hex::encode(&key_bytes))
            .and_then(|item| item.value.as_deref())
            .and_then(|value| hex::decode(value.trim_start_matches("0x")).ok())
        else {
            return Ok(receipt);
        };

        let records = scales::Value::new(&raw, key.ty, &metadata.registry);
        let Some(records) = records.sequence_iter() else {
            return Ok(receipt);
        };
        for record in records {
            let Some(phase) = record.field("phase") else {
                continue;
            };
            if phase.variant_name() != Some("ApplyExtrinsic")
                || phase.variant_data().and_then(|value| value.as_u32()) != Some(extrinsic_index)
            {
                continue;
            }

            let Some(event) = record.field("event") else {
                continue;
            };
            let Some(pallet) = event.variant_name() else {
                continue;
            };
            let Some(pallet_event) = event.variant_data() else {
                continue;
            };
            let Some(variant) = pallet_event.variant_name() else {
                continue;
            };

            if pallet.eq_ignore_ascii_case("System") && variant == "ExtrinsicSuccess" {
                receipt.dispatch_outcome = crate::DispatchOutcome::Success;
            } else if pallet.eq_ignore_ascii_case("System") && variant == "ExtrinsicFailed" {
                let error = resolve_dispatch_error(&pallet_event, metadata).unwrap_or_else(|| {
                    scales::to_text(&pallet_event)
                        .ok()
                        .unwrap_or_else(|| "runtime dispatch error".into())
                });
                receipt.dispatch_outcome = crate::DispatchOutcome::Failed(error);
            }

            receipt.events.push(crate::TransactionEvent {
                pallet: pallet.into(),
                variant: variant.into(),
                data: Vec::new(),
                decoded: scales::to_text(&pallet_event).ok(),
            });
        }

        Ok(receipt)
    }

    async fn chain_properties(&mut self) -> crate::Result<crate::ChainProperties> {
        let raw = self
            .rpc
            .rpc("system_properties", "[]")
            .await
            .map_err(|error| crate::Error::Node(error.to_string()))?;
        let ss58_format = property_value(&raw, "ss58Format")
            .and_then(|value| parse_unsigned_values(&value).into_iter().next())
            .and_then(|value| u16::try_from(value).ok());
        let token_symbols = property_value(&raw, "tokenSymbol")
            .map(|value| parse_string_values(&value))
            .unwrap_or_default();
        let token_decimals = property_value(&raw, "tokenDecimals")
            .map(|value| parse_unsigned_values(&value))
            .unwrap_or_default();
        Ok(crate::ChainProperties {
            ss58_format,
            token_symbols,
            token_decimals,
        })
    }

    async fn metadata(&mut self) -> crate::Result<Metadata> {
        let raw = self.fetch_raw_metadata().await?;
        meta::from_bytes(&raw)
    }

    async fn metadata_at_hash(&mut self, block_hash: [u8; 32]) -> crate::Result<Metadata> {
        self.legacy_metadata_at_hash(&block_hash).await
    }

    async fn block_info(&mut self, at: Option<u32>) -> crate::Result<meta::BlockInfo> {
        match at {
            Some(0) => Ok(meta::BlockInfo {
                number: 0,
                hash: self.genesis_hash,
                parent: self.genesis_hash,
            }),
            None => {
                // A follow subscription can be stopped while the caller is
                // decoding a large metadata response. Drain lifecycle events
                // and, if the stop races this header request, re-follow once
                // and use the newly pinned finalized block.
                for _ in 0..2 {
                    let finalized_hash = self.prepare_operation().await?;
                    match self.header(&finalized_hash).await {
                        Ok(header) => {
                            let mut h = [0u8; 32];
                            hex::decode_to_slice(finalized_hash.trim_start_matches("0x"), &mut h)
                                .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;
                            let mut parent = [0u8; 32];
                            hex::decode_to_slice(
                                header.parent_hash.trim_start_matches("0x"),
                                &mut parent,
                            )
                            .map_err(|_| {
                                crate::Error::Decode("parent hash hex decode failed".into())
                            })?;
                            return Ok(meta::BlockInfo {
                                number: header.number,
                                hash: h,
                                parent,
                            });
                        }
                        Err(crate::Error::SubscriptionClosed) => {
                            self.needs_refollow = true;
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(crate::Error::SubscriptionClosed)
            }
            Some(n) => {
                let hash_hex = self.archive_hash_by_height(n as u64).await?;
                let mut h = [0u8; 32];
                hex::decode_to_slice(hash_hex.trim_start_matches("0x"), &mut h)
                    .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;
                Ok(meta::BlockInfo {
                    number: n as u64,
                    hash: h,
                    parent: h, // parent not resolved — would need archive_v1_header
                })
            }
        }
    }

    async fn block_info_at_hash(&mut self, block_hash: [u8; 32]) -> crate::Result<meta::BlockInfo> {
        self.legacy_block_info_at_hash(&block_hash).await
    }
}

fn property_value(json: &str, name: &str) -> Option<String> {
    extract_json_object(json, &format!("\"{name}\":"))
}

fn parse_unsigned_values(raw: &str) -> Vec<u32> {
    raw.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|value| value.trim().parse().ok())
        .collect()
}

fn parse_string_values(raw: &str) -> Vec<String> {
    raw.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|value| super::parse_json_string(value.trim()))
        .collect()
}

fn resolve_dispatch_error(failed_event: &scales::Value<'_>, metadata: &Metadata) -> Option<String> {
    let error = failed_event.variant_field_at(0)?;
    let variant = error.variant_name()?;
    if variant != "Module" {
        return Some(variant.into());
    }

    let module = error.variant_field_at(0)?;
    let pallet_index = module
        .field("index")
        .or_else(|| module.field_at(0))?
        .as_u8()?;
    let encoded_error = module.field("error").or_else(|| module.field_at(1))?;
    let error_index = encoded_error
        .array_get(0)
        .and_then(|value| value.as_u8())
        .or_else(|| encoded_error.as_u8())?;
    resolve_module_error(metadata, pallet_index, error_index)
}

fn resolve_module_error(metadata: &Metadata, pallet_index: u8, error_index: u8) -> Option<String> {
    let pallet = metadata
        .pallets
        .iter()
        .find(|pallet| pallet.index == pallet_index)?;
    let errors_ty = pallet.errors_ty?;
    let scales::TypeDef::Variant(errors) = metadata.registry.resolve(errors_ty)? else {
        return None;
    };
    let error = errors.variant(error_index).ok()?;
    Some(format!("{}::{}", pallet.name, error.name()))
}

// --- Two-request metadata fetch helpers ---

impl<R: Rpc + RpcSubscription> ChainHead<R> {
    /// Fetch raw metadata bytes, stripping the compact length prefix.
    async fn fetch_raw_metadata(&mut self) -> crate::Result<Vec<u8>> {
        let raw = self.runtime_call("Metadata_metadata", "0x").await?;
        let mut cursor = raw.as_slice();
        let _len = <codec::Compact<u32>>::decode(&mut cursor)
            .map_err(|_| crate::Error::Decode("compact prefix".into()))?;
        Ok(cursor.to_vec())
    }

    /// Scan pallet names from metadata without decoding types.
    pub async fn scan_pallets(&mut self) -> crate::Result<Vec<String>> {
        let raw = self.fetch_raw_metadata().await?;
        let pallets =
            scales::frame::metadata::scan_pallets(&raw).map_err(|_| crate::Error::BadMetadata)?;
        Ok(pallets.into_iter().map(|p| p.name).collect())
    }

    /// Fetch metadata keeping only the specified pallets and their referenced types.
    pub async fn metadata_filtered(&mut self, pallets: &[&str]) -> crate::Result<Metadata> {
        let raw = self.fetch_raw_metadata().await?;
        meta::from_bytes_filtered(&raw, pallets)
    }
}

// --- Streaming metadata (edge backend, memory-constrained) ---

#[cfg(feature = "ws-edge")]
impl<T: embedded_io_async::Read + embedded_io_async::Write> ChainHead<super::edge::Backend<T>> {
    /// Fetch filtered metadata via two streaming passes.
    ///
    /// Pass 1: scan type references + decode pallets (~35KB peak).
    /// Pass 2: decode only needed types (~80KB peak).
    /// Never holds the full metadata blob in memory.
    /// Pass 1: scan pallets from metadata, return needed type IDs + type count.
    /// Drop the connection after this to free heap for pass 2.
    pub async fn metadata_scan_pallets(
        &mut self,
        pallet_filter: &[&str],
    ) -> crate::Result<(alloc::collections::BTreeSet<u32>, u32)> {
        use scales::frame::streaming_metadata;

        log::info!("metadata: pass 1 — scanning pallets");
        self.send_runtime_call("Metadata_metadata", "0x").await?;
        let scan = {
            let mut hex_reader = super::edge::HexFrameReader::new(&mut self.rpc);
            skip_opaque_prefix(&mut hex_reader).await?;
            let result = Box::pin(streaming_metadata::scan_pallets_streaming(
                &mut hex_reader,
                pallet_filter,
            ))
            .await
            .map_err(|e| crate::Error::Decode(alloc::format!("scan: {e}")))?;
            hex_reader.finish().await;
            result
        };
        log::info!(
            "metadata: {} pallets, {} types",
            scan.pallets.len(),
            scan.type_count
        );

        let root = scales::frame::metadata::collect_storage_type_ids(&scan.pallets);
        let mut needed = alloc::collections::BTreeSet::new();
        for id in root {
            needed.insert(id);
        }
        log::info!("metadata: {} root type IDs", needed.len());
        Ok((needed, scan.type_count))
    }

    /// Pass 2+3: decode filtered types and re-decode pallets.
    /// Call on a FRESH connection (after dropping pass 1's connection to free heap).
    pub async fn metadata_decode_filtered(
        &mut self,
        pallet_filter: &[&str],
        needed_ids: &alloc::collections::BTreeSet<u32>,
        type_count: u32,
        registry: &mut scales::Registry,
    ) -> crate::Result<Metadata> {
        use scales::frame::streaming_metadata;

        // Pass 2: decode types into pre-allocated registry
        log::info!("metadata: pass 2 — decoding filtered types");
        self.send_runtime_call("Metadata_metadata", "0x").await?;
        let id_map = {
            let mut hex_reader = super::edge::HexFrameReader::new(&mut self.rpc);
            skip_opaque_prefix(&mut hex_reader).await?;
            let result = Box::pin(streaming_metadata::decode_filtered_to_registry(
                &mut hex_reader,
                needed_ids,
                type_count,
                registry,
            ))
            .await
            .map_err(|e| crate::Error::Decode(alloc::format!("decode: {e}")))?;
            hex_reader.finish().await;
            result
        };
        registry.remap_ids(&|id| id_map.get(&id).copied().unwrap_or(id));
        registry.postprocess();
        log::info!("metadata: registry built");

        // Pass 3: re-decode pallets
        log::info!("metadata: pass 3 — re-decoding pallets");
        self.send_runtime_call("Metadata_metadata", "0x").await?;
        let scan = {
            let mut hex_reader = super::edge::HexFrameReader::new(&mut self.rpc);
            skip_opaque_prefix(&mut hex_reader).await?;
            let result = Box::pin(streaming_metadata::scan_pallets_streaming(
                &mut hex_reader,
                pallet_filter,
            ))
            .await
            .map_err(|e| crate::Error::Decode(alloc::format!("rescan: {e}")))?;
            hex_reader.finish().await;
            result
        };

        let remap = |id: u32| id_map.get(&id).copied().unwrap_or(id);
        let pallets = scales::frame::metadata::remap_pallet_ids(scan.pallets, &remap);
        let extrinsic = scales::frame::metadata::remap_extrinsic_ids(scan.extrinsic, &remap);
        log::info!("metadata: ready ({} pallets)", pallets.len());
        // Take the filled registry out, replacing with empty
        let built_registry = core::mem::replace(registry, scales::Registry::with_capacity(0));
        Ok(meta::from_raw(pallets, extrinsic, built_registry))
    }

    /// Send a runtime call RPC request without reading the response.
    ///
    /// The response and the operationCallDone notification will both
    /// arrive as WebSocket frames — HexFrameReader handles them,
    /// skipping the non-hex response and processing the hex notification.
    async fn send_runtime_call(&mut self, function: &str, call_data: &str) -> crate::Result<()> {
        // Use finalized hash directly — don't flush unpins (which calls rpc()
        // and could try to parse a large buffered notification, blowing the stack).
        let hash = self.finalized_hash.clone();
        let id = self.rpc.next_id;
        self.rpc.next_id += 1;
        log::info!("RPC `chainHead_v1_call` (ID={})", id);

        let mut msg = String::new();
        super::format_request(
            &mut msg,
            id,
            "chainHead_v1_call",
            &alloc::format!(
                r#"["{}","{}","{}","{}"]"#,
                self.follow_sub_id,
                hash,
                function,
                call_data
            ),
        );
        self.rpc
            .send_text(msg.as_bytes())
            .await
            .map_err(|e| crate::Error::Node(alloc::format!("rpc send: {e}")))?;
        Ok(())
    }
}

#[cfg(feature = "ws-edge")]
async fn skip_opaque_prefix<R: embedded_io_async::Read>(reader: &mut R) -> crate::Result<()> {
    // Read and skip the compact u32 length prefix of OpaqueMetadata.
    let mut b = [0u8; 1];
    reader
        .read_exact(&mut b)
        .await
        .map_err(|_| crate::Error::Decode("opaque prefix read".into()))?;
    let mode = b[0] & 0x03;
    let skip = match mode {
        0 => 0,
        1 => 1,
        2 => 3,
        _ => ((b[0] >> 2) + 4) as usize,
    };
    log::debug!(
        "opaque prefix: byte=0x{:02x} mode={} skip={}",
        b[0],
        mode,
        skip
    );
    for _ in 0..skip {
        reader
            .read_exact(&mut b)
            .await
            .map_err(|_| crate::Error::Decode("opaque prefix skip".into()))?;
    }
    Ok(())
}

// --- ChainSession trait ---

/// Extended backend operations available when a ChainHead subscription is active.
///
/// Implemented by [`ChainHead<R>`] and [`AnyBackend`](crate::backend::AnyBackend).
/// This trait is what allows [`Sube<B>`](crate::Sube) to provide chain-event
/// streaming and block-pinned queries generically.
#[allow(async_fn_in_trait)]
pub trait ChainSession: crate::Backend {
    fn retain_block(&mut self, block_hash: &str);
    fn release_block(&mut self, block_hash: &str);
    async fn next_chain_event(&mut self) -> crate::Result<ChainEvent>;
    fn try_next_chain_event(&mut self) -> Option<ChainEvent>;
    async fn header(&mut self, block_hash: &str) -> crate::Result<BlockHeader>;
    async fn runtime_call_at(
        &mut self,
        block_hash: &str,
        function: &str,
        call_data: &str,
    ) -> crate::Result<Vec<u8>>;
    async fn scan_pallets(&mut self) -> crate::Result<Vec<String>>;
    async fn metadata_filtered(&mut self, pallets: &[&str]) -> crate::Result<Metadata>;
    async fn get_storage_at_hash(
        &mut self,
        block_hash: &str,
        keys: Vec<crate::RawKey>,
    ) -> crate::Result<Vec<(crate::RawKey, Option<crate::RawValue>)>>;
}

impl<R: Rpc + RpcSubscription> ChainSession for ChainHead<R> {
    fn retain_block(&mut self, block_hash: &str) {
        self.retain_block(block_hash);
    }
    fn release_block(&mut self, block_hash: &str) {
        self.release_block(block_hash);
    }
    async fn next_chain_event(&mut self) -> crate::Result<ChainEvent> {
        self.next_chain_event().await
    }
    fn try_next_chain_event(&mut self) -> Option<ChainEvent> {
        self.try_next_chain_event()
    }
    async fn header(&mut self, block_hash: &str) -> crate::Result<BlockHeader> {
        self.header(block_hash).await
    }
    async fn runtime_call_at(
        &mut self,
        block_hash: &str,
        function: &str,
        call_data: &str,
    ) -> crate::Result<Vec<u8>> {
        self.runtime_call_at(block_hash, function, call_data).await
    }
    async fn scan_pallets(&mut self) -> crate::Result<Vec<String>> {
        self.scan_pallets().await
    }
    async fn metadata_filtered(&mut self, pallets: &[&str]) -> crate::Result<Metadata> {
        self.metadata_filtered(pallets).await
    }
    async fn get_storage_at_hash(
        &mut self,
        block_hash: &str,
        keys: Vec<crate::RawKey>,
    ) -> crate::Result<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        self.get_storage_at_hash(block_hash, keys).await
    }
}

#[cfg(test)]
mod transaction_watch_tests {
    use super::*;

    #[test]
    fn operation_started_preserves_discarded_item_count() {
        assert!(matches!(
            parse_operation_started(
                r#"{"result":"started","operationId":"op","discardedItems":2}"#
            )
            .unwrap(),
            OperationStarted::Started {
                operation_id: "op",
                discarded_items: 2
            }
        ));
    }

    struct DiscardedStorageRpc {
        stopped: bool,
    }

    impl Rpc for DiscardedStorageRpc {
        async fn rpc(&mut self, method: &str, _params: &str) -> super::super::RpcResult<String> {
            match method {
                "chainHead_v1_storage" => Ok(
                    r#"{"result":"started","operationId":"discarded","discardedItems":1}"#.into(),
                ),
                "chainHead_v1_stopOperation" => {
                    self.stopped = true;
                    Ok("null".into())
                }
                other => panic!("unexpected RPC method: {other}"),
            }
        }
    }

    impl RpcSubscription for DiscardedStorageRpc {
        async fn subscribe(
            &mut self,
            _method: &str,
            _params: &str,
        ) -> super::super::RpcResult<String> {
            unreachable!()
        }

        async fn next_event(&mut self) -> Option<(String, String)> {
            panic!("discarded operation must not be awaited")
        }

        fn try_next_event(&mut self) -> Option<(String, String)> {
            None
        }

        async fn unsubscribe(
            &mut self,
            _method: &str,
            _sub_id: &str,
        ) -> super::super::RpcResult<()> {
            unreachable!()
        }
    }

    #[test]
    fn discarded_storage_item_stops_the_operation_and_fails_closed() {
        smol::block_on(async {
            let mut chain = ChainHead {
                rpc: DiscardedStorageRpc { stopped: false },
                follow_sub_id: "follow".into(),
                finalized_hash: to_hex(&[0x11; 32]),
                genesis_hash: [0; 32],
                storage_accum: BTreeMap::new(),
                pending_unpin: Vec::new(),
                needs_refollow: false,
                retained_hashes: BTreeMap::new(),
                event_queue: VecDeque::new(),
                active_operation: None,
                active_tx_watch: None,
            };
            let error = chain
                .storage_one_with_hash(&to_hex(&[0x11; 32]), "0xaabb")
                .await
                .unwrap_err();
            assert!(matches!(error, crate::Error::OperationFailed(_)));
            assert!(chain.rpc.stopped);
        });
    }

    #[cfg(feature = "std")]
    struct PendingKeyRpc {
        stopped: usize,
    }

    #[cfg(feature = "std")]
    impl Rpc for PendingKeyRpc {
        async fn rpc(&mut self, method: &str, _params: &str) -> super::super::RpcResult<String> {
            match method {
                "chainHead_v1_storage" => Ok(
                    r#"{"result":"started","operationId":"pending-key","discardedItems":0}"#.into(),
                ),
                "chainHead_v1_stopOperation" => {
                    self.stopped += 1;
                    Ok("null".into())
                }
                other => panic!("unexpected RPC method: {other}"),
            }
        }
    }

    #[cfg(feature = "std")]
    impl RpcSubscription for PendingKeyRpc {
        async fn subscribe(
            &mut self,
            _method: &str,
            _params: &str,
        ) -> super::super::RpcResult<String> {
            unreachable!()
        }

        async fn next_event(&mut self) -> Option<(String, String)> {
            core::future::pending().await
        }

        fn try_next_event(&mut self) -> Option<(String, String)> {
            None
        }

        async fn unsubscribe(
            &mut self,
            _method: &str,
            _sub_id: &str,
        ) -> super::super::RpcResult<()> {
            unreachable!()
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn cancelled_key_page_leaves_an_explicitly_stoppable_operation() {
        smol::block_on(async {
            let mut chain = ChainHead {
                rpc: PendingKeyRpc { stopped: 0 },
                follow_sub_id: "follow".into(),
                finalized_hash: to_hex(&[0x11; 32]),
                genesis_hash: [0; 32],
                storage_accum: BTreeMap::new(),
                pending_unpin: Vec::new(),
                needs_refollow: false,
                retained_hashes: BTreeMap::new(),
                event_queue: VecDeque::new(),
                active_operation: None,
                active_tx_watch: None,
            };
            let timed_out = crate::time::timeout(
                core::time::Duration::from_millis(10),
                chain.key_item_operation_at_hash("0x11", &[0xaa], "hash", None, 1),
            )
            .await;
            assert!(timed_out.is_err());
            assert_eq!(chain.active_operation.as_deref(), Some("pending-key"));
            chain.cancel_active_operation().await.unwrap();
            assert!(chain.active_operation.is_none());
            assert_eq!(chain.rpc.stopped, 1);
        });
    }

    #[test]
    fn light_page_cursor_round_trips_partition_and_last_key() {
        let prefix = [0xaa, 0xbb];
        let partition = [0x00, 0x42, 0x07];
        let after = [0xaa, 0xbb, 0x00, 0x42, 0x07, 0x99];
        let encoded = encode_light_page_cursor(&partition, Some(&after)).unwrap();
        let (decoded_partition, decoded_after) =
            decode_light_page_cursor(&prefix, Some(encoded)).unwrap();
        assert_eq!(decoded_partition, partition);
        assert_eq!(decoded_after.as_deref(), Some(after.as_slice()));
    }

    #[test]
    fn light_page_cursor_rejects_a_last_key_outside_its_partition() {
        let encoded = encode_light_page_cursor(&[0x01, 0x02], Some(&[0xaa, 0x01, 0x03])).unwrap();
        assert!(decode_light_page_cursor(&[0xaa], Some(encoded)).is_err());
    }

    #[test]
    fn light_page_partition_advances_depth_first_without_overlap() {
        let mut partition = vec![0x00, 0x01, 0xfe];
        assert!(advance_light_page_partition(&mut partition));
        assert_eq!(partition, [0x00, 0x01, 0xff]);
        assert!(advance_light_page_partition(&mut partition));
        assert_eq!(partition, [0x00, 0x02]);
        partition = vec![0xff, 0xff];
        assert!(!advance_light_page_partition(&mut partition));
    }

    #[test]
    fn storage_items_distinguish_hashes_from_descendant_probes() {
        let items = parse_storage_items(
            r#"{"items":[{"key":"0x01","hash":"0x02"},{"key":"0x01","closestDescendantMerkleValue":"0x03"}]}"#,
        );
        assert_eq!(items.len(), 2);
        assert!(items[0].hash.is_some());
        assert!(items[0].closest_descendant_merkle_value.is_none());
        assert!(items[1].hash.is_none());
        assert!(items[1].closest_descendant_merkle_value.is_some());
    }

    struct UnorderedKeysRpc {
        operation: usize,
        incoming: VecDeque<(String, String)>,
    }

    impl Rpc for UnorderedKeysRpc {
        async fn rpc(&mut self, method: &str, params: &str) -> super::super::RpcResult<String> {
            match method {
                "chainHead_v1_storage" => {
                    let operation_id = format!("op{}", self.operation);
                    let keys = if params.contains("descendantsHashes") {
                        [0x30u8, 0x20, 0x10]
                            .into_iter()
                            .map(|suffix| {
                                format!(
                                    r#"{{"key":"0xaa{suffix:02x}","hash":"0x{}"}}"#,
                                    "11".repeat(32)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    } else {
                        String::new()
                    };
                    self.incoming.push_back((
                        "follow".into(),
                        format!(
                            r#"{{"event":"operationStorageItems","operationId":"{operation_id}","items":[{keys}]}}"#
                        ),
                    ));
                    self.incoming.push_back((
                        "follow".into(),
                        format!(
                            r#"{{"event":"operationStorageDone","operationId":"{operation_id}"}}"#
                        ),
                    ));
                    self.operation += 1;
                    Ok(format!(
                        r#"{{"result":"started","operationId":"{operation_id}","discardedItems":0}}"#
                    ))
                }
                other => panic!("unexpected RPC method: {other}"),
            }
        }
    }

    impl RpcSubscription for UnorderedKeysRpc {
        async fn subscribe(
            &mut self,
            _method: &str,
            _params: &str,
        ) -> super::super::RpcResult<String> {
            unreachable!()
        }

        async fn next_event(&mut self) -> Option<(String, String)> {
            self.incoming.pop_front()
        }

        fn try_next_event(&mut self) -> Option<(String, String)> {
            None
        }

        async fn unsubscribe(
            &mut self,
            _method: &str,
            _sub_id: &str,
        ) -> super::super::RpcResult<()> {
            unreachable!()
        }
    }

    #[test]
    fn unordered_descendants_are_sorted_before_cursoring() {
        smol::block_on(async {
            let hash = [0x55; 32];
            let hash_hex = to_hex(&hash);
            let mut retained_hashes = BTreeMap::new();
            retained_hashes.insert(hash_hex.clone(), 1);
            let mut chain = ChainHead {
                rpc: UnorderedKeysRpc {
                    operation: 0,
                    incoming: VecDeque::new(),
                },
                follow_sub_id: "follow".into(),
                finalized_hash: hash_hex,
                genesis_hash: [0; 32],
                storage_accum: BTreeMap::new(),
                pending_unpin: Vec::new(),
                needs_refollow: false,
                retained_hashes,
                event_queue: VecDeque::new(),
                active_operation: None,
                active_tx_watch: None,
            };

            let first = chain
                .partitioned_keys_page_at_hash(&[0xaa], 2, None, &hash)
                .await
                .unwrap();
            assert_eq!(first.keys, [vec![0xaa, 0x10], vec![0xaa, 0x20]]);
            let second = chain
                .partitioned_keys_page_at_hash(&[0xaa], 2, first.next_cursor, &hash)
                .await
                .unwrap();
            assert_eq!(second.keys, [vec![0xaa, 0x30]]);
            assert!(second.next_cursor.is_none());
        });
    }

    struct RecoveryRpc {
        buffered: VecDeque<(String, String)>,
        incoming: VecDeque<(String, String)>,
        subscribe_calls: usize,
    }

    impl Rpc for RecoveryRpc {
        async fn rpc(&mut self, method: &str, params: &str) -> super::super::RpcResult<String> {
            assert_eq!(method, "chainHead_v1_header");
            assert!(params.contains("new-follow"));
            assert!(params.contains(&format!("0x{}", "22".repeat(32))));
            let header = format!(
                "\"0x{}a8{}{}\"",
                "11".repeat(32),
                "33".repeat(32),
                "44".repeat(32)
            );
            Ok(header)
        }
    }

    impl RpcSubscription for RecoveryRpc {
        async fn subscribe(
            &mut self,
            method: &str,
            _params: &str,
        ) -> super::super::RpcResult<String> {
            assert_eq!(method, "chainHead_v1_follow");
            self.subscribe_calls += 1;
            Ok("new-follow".into())
        }

        async fn next_event(&mut self) -> Option<(String, String)> {
            self.incoming.pop_front()
        }

        fn try_next_event(&mut self) -> Option<(String, String)> {
            self.buffered.pop_front()
        }

        async fn unsubscribe(&mut self, method: &str, sub_id: &str) -> super::super::RpcResult<()> {
            assert_eq!(method, "chainHead_v1_unfollow");
            assert_eq!(sub_id, "stopped-follow");
            Ok(())
        }
    }

    #[test]
    fn finalized_block_recovers_a_stopped_follow_subscription() {
        let new_hash = format!("0x{}", "22".repeat(32));
        let rpc = RecoveryRpc {
            buffered: [("stopped-follow".into(), r#"{"event":"stop"}"#.into())].into(),
            incoming: [(
                "new-follow".into(),
                format!(r#"{{"event":"initialized","finalizedBlockHashes":["{new_hash}"]}}"#),
            )]
            .into(),
            subscribe_calls: 0,
        };
        let mut chain = ChainHead {
            rpc,
            follow_sub_id: "stopped-follow".into(),
            finalized_hash: format!("0x{}", "aa".repeat(32)),
            genesis_hash: [0; 32],
            storage_accum: BTreeMap::new(),
            pending_unpin: Vec::new(),
            needs_refollow: false,
            retained_hashes: BTreeMap::new(),
            event_queue: VecDeque::new(),
            active_operation: None,
            active_tx_watch: None,
        };

        let info = smol::block_on(crate::Backend::block_info(&mut chain, None)).unwrap();
        assert_eq!(info.number, 42);
        assert_eq!(info.hash, [0x22; 32]);
        assert_eq!(info.parent, [0x11; 32]);
        assert_eq!(chain.rpc.subscribe_calls, 1);
    }

    #[test]
    fn parses_best_inclusion_and_retraction() {
        let included = r#"{"event":"bestChainBlockIncluded","block":{"hash":"0xabc","index":3}}"#;
        match parse_tx_event(included).unwrap() {
            TxEvent::BestChainBlockIncluded { block: Some(block) } => {
                assert_eq!(block.hash, "0xabc");
                assert_eq!(block.index, 3);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let retracted = r#"{"event":"bestChainBlockIncluded","block":null}"#;
        assert!(matches!(
            parse_tx_event(retracted).unwrap(),
            TxEvent::BestChainBlockIncluded { block: None }
        ));
    }

    #[test]
    fn finalized_requires_and_preserves_location() {
        let event = r#"{"event":"finalized","block":{"hash":"0xdef","index":7}}"#;
        match parse_tx_event(event).unwrap() {
            TxEvent::Finalized { block } => {
                assert_eq!(block.hash, "0xdef");
                assert_eq!(block.index, 7);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(parse_tx_event(r#"{"event":"finalized"}"#).is_err());
    }

    struct BestBlockWatchRpc {
        incoming: VecDeque<(String, String)>,
        unwatched: usize,
    }

    impl Rpc for BestBlockWatchRpc {
        async fn rpc(&mut self, method: &str, _params: &str) -> super::super::RpcResult<String> {
            panic!("unexpected RPC call: {method}")
        }
    }

    impl RpcSubscription for BestBlockWatchRpc {
        async fn subscribe(
            &mut self,
            method: &str,
            _params: &str,
        ) -> super::super::RpcResult<String> {
            assert_eq!(method, "transactionWatch_v1_submitAndWatch");
            Ok("tx-watch".into())
        }

        async fn next_event(&mut self) -> Option<(String, String)> {
            self.incoming.pop_front()
        }

        fn try_next_event(&mut self) -> Option<(String, String)> {
            None
        }

        async fn unsubscribe(&mut self, method: &str, sub_id: &str) -> super::super::RpcResult<()> {
            assert_eq!(method, "transactionWatch_v1_unwatch");
            assert_eq!(sub_id, "tx-watch");
            self.unwatched += 1;
            Ok(())
        }
    }

    #[test]
    fn best_block_result_explicitly_unwatches_subscription() {
        smol::block_on(async {
            let rpc = BestBlockWatchRpc {
                incoming: [(
                    "tx-watch".into(),
                    r#"{"event":"bestChainBlockIncluded","block":{"hash":"0xabc","index":3}}"#
                        .into(),
                )]
                .into(),
                unwatched: 0,
            };
            let mut chain = ChainHead {
                rpc,
                follow_sub_id: "follow".into(),
                finalized_hash: format!("0x{}", "55".repeat(32)),
                genesis_hash: [0; 32],
                storage_accum: BTreeMap::new(),
                pending_unpin: Vec::new(),
                needs_refollow: false,
                retained_hashes: BTreeMap::new(),
                event_queue: VecDeque::new(),
                active_operation: None,
                active_tx_watch: None,
            };
            let receipt = chain
                .watch_transaction(&[1, 2, 3], crate::WaitFor::BestBlock, None)
                .await
                .unwrap();
            assert_eq!(receipt.best_block_hash.as_deref(), Some("0xabc"));
            assert_eq!(chain.rpc.unwatched, 1);
            assert!(chain.active_tx_watch.is_none());
        });
    }

    #[cfg(feature = "std")]
    struct TimeoutWatchRpc {
        unwatched: usize,
    }

    #[cfg(feature = "std")]
    impl Rpc for TimeoutWatchRpc {
        async fn rpc(&mut self, method: &str, _params: &str) -> super::super::RpcResult<String> {
            panic!("unexpected RPC call: {method}")
        }
    }

    #[cfg(feature = "std")]
    impl RpcSubscription for TimeoutWatchRpc {
        async fn subscribe(
            &mut self,
            _method: &str,
            _params: &str,
        ) -> super::super::RpcResult<String> {
            Ok("tx-watch".into())
        }

        async fn next_event(&mut self) -> Option<(String, String)> {
            core::future::pending().await
        }

        fn try_next_event(&mut self) -> Option<(String, String)> {
            None
        }

        async fn unsubscribe(&mut self, method: &str, sub_id: &str) -> super::super::RpcResult<()> {
            assert_eq!(method, "transactionWatch_v1_unwatch");
            assert_eq!(sub_id, "tx-watch");
            self.unwatched += 1;
            Ok(())
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn transaction_timeout_explicitly_unwatches_subscription() {
        smol::block_on(async {
            let mut chain = ChainHead {
                rpc: TimeoutWatchRpc { unwatched: 0 },
                follow_sub_id: "follow".into(),
                finalized_hash: to_hex(&[0x55; 32]),
                genesis_hash: [0; 32],
                storage_accum: BTreeMap::new(),
                pending_unpin: Vec::new(),
                needs_refollow: false,
                retained_hashes: BTreeMap::new(),
                event_queue: VecDeque::new(),
                active_operation: None,
                active_tx_watch: None,
            };
            let error = chain
                .watch_transaction(
                    &[1, 2, 3],
                    crate::WaitFor::Finalized,
                    Some(core::time::Duration::from_millis(10)),
                )
                .await
                .unwrap_err();
            assert!(matches!(error, crate::Error::ConnectionTimeout));
            assert_eq!(chain.rpc.unwatched, 1);
            assert!(chain.active_tx_watch.is_none());
        });
    }

    #[cfg(feature = "std")]
    #[test]
    fn cancelled_transaction_watch_remains_explicitly_unwatchable() {
        smol::block_on(async {
            let mut chain = ChainHead {
                rpc: TimeoutWatchRpc { unwatched: 0 },
                follow_sub_id: "follow".into(),
                finalized_hash: to_hex(&[0x55; 32]),
                genesis_hash: [0; 32],
                storage_accum: BTreeMap::new(),
                pending_unpin: Vec::new(),
                needs_refollow: false,
                retained_hashes: BTreeMap::new(),
                event_queue: VecDeque::new(),
                active_operation: None,
                active_tx_watch: None,
            };
            let result = crate::time::timeout(
                core::time::Duration::from_millis(10),
                chain.watch_transaction(&[1, 2, 3], crate::WaitFor::Finalized, None),
            )
            .await;
            assert!(result.is_err());
            assert_eq!(chain.active_tx_watch.as_deref(), Some("tx-watch"));
            chain.cancel_active_operation().await.unwrap();
            assert_eq!(chain.rpc.unwatched, 1);
            assert!(chain.active_tx_watch.is_none());
        });
    }

    struct LegacyBlockRpc;

    impl Rpc for LegacyBlockRpc {
        async fn rpc(&mut self, method: &str, params: &str) -> super::super::RpcResult<String> {
            assert!(params.contains(&format!("0x{}", "77".repeat(32))));
            match method {
                "state_getMetadata" => Ok(format!(
                    "\"0x{}\"",
                    hex::encode(include_bytes!("../../tests/fixtures/kreivo.scale"))
                )),
                "chain_getHeader" => Ok(format!(
                    r#"{{"parentHash":"0x{}","number":"0x2a","stateRoot":"0x{}","extrinsicsRoot":"0x{}"}}"#,
                    "66".repeat(32),
                    "55".repeat(32),
                    "44".repeat(32)
                )),
                other => panic!("unexpected RPC method: {other}"),
            }
        }
    }

    impl RpcSubscription for LegacyBlockRpc {
        async fn subscribe(
            &mut self,
            _method: &str,
            _params: &str,
        ) -> super::super::RpcResult<String> {
            unreachable!()
        }

        async fn next_event(&mut self) -> Option<(String, String)> {
            unreachable!()
        }

        fn try_next_event(&mut self) -> Option<(String, String)> {
            None
        }

        async fn unsubscribe(
            &mut self,
            _method: &str,
            _sub_id: &str,
        ) -> super::super::RpcResult<()> {
            unreachable!()
        }
    }

    #[test]
    fn metadata_and_block_number_are_resolved_at_the_supplied_hash() {
        smol::block_on(async {
            let hash = [0x77; 32];
            let mut chain = ChainHead {
                rpc: LegacyBlockRpc,
                follow_sub_id: "follow".into(),
                finalized_hash: to_hex(&hash),
                genesis_hash: [0; 32],
                storage_accum: BTreeMap::new(),
                pending_unpin: Vec::new(),
                needs_refollow: false,
                retained_hashes: BTreeMap::new(),
                event_queue: VecDeque::new(),
                active_operation: None,
                active_tx_watch: None,
            };
            let metadata = crate::Backend::metadata_at_hash(&mut chain, hash)
                .await
                .unwrap();
            assert!(metadata.pallet_by_name("System").is_some());
            let block = crate::Backend::block_info_at_hash(&mut chain, hash)
                .await
                .unwrap();
            assert_eq!(block.number, 42);
            assert_eq!(block.hash, hash);
            assert_eq!(block.parent, [0x66; 32]);
        });
    }

    #[test]
    fn module_errors_resolve_through_pallet_metadata() {
        let metadata =
            Metadata::from_bytes(include_bytes!("../../tests/fixtures/kreivo.scale")).unwrap();
        let pallet = metadata
            .pallet_by_name("Balances")
            .expect("Balances pallet");
        let errors_ty = pallet.errors_ty.expect("Balances errors");
        let scales::TypeDef::Variant(errors) =
            metadata.registry.resolve(errors_ty).expect("error type")
        else {
            panic!("pallet errors are not an enum");
        };
        let first = errors.variants().next().expect("at least one error");
        assert_eq!(
            resolve_module_error(&metadata, pallet.index, first.index()),
            Some(format!("Balances::{}", first.name()))
        );
    }

    #[test]
    fn parses_scalar_and_multi_token_chain_properties() {
        let properties = r#"{"ss58Format":2,"tokenSymbol":["KSM","USDT"],"tokenDecimals":[12,6]}"#;
        assert_eq!(
            property_value(properties, "ss58Format").map(|value| parse_unsigned_values(&value)),
            Some(vec![2])
        );
        assert_eq!(
            property_value(properties, "tokenSymbol").map(|value| parse_string_values(&value)),
            Some(vec!["KSM".into(), "USDT".into()])
        );
        assert_eq!(
            property_value(properties, "tokenDecimals").map(|value| parse_unsigned_values(&value)),
            Some(vec![12, 6])
        );
    }
}
