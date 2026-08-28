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
    /// User-visible events buffered during internal operations.
    event_queue: VecDeque<ChainEvent>,
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
/// Each item is `{"key":"0x...","value":"0x..."}` with optional value.
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
                    items.push(StorageItem { key, value });
                }
            }
            _ => {}
        }
    }
    items
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
    Started { operation_id: &'a str },
    LimitReached,
}

fn parse_operation_started(json: &str) -> Result<OperationStarted<'_>, crate::Error> {
    let result = extract_json_str(json, "\"result\":\"")
        .ok_or_else(|| crate::Error::Decode("missing result in operation response".into()))?;
    match result {
        "started" => Ok(OperationStarted::Started {
            operation_id: extract_json_str(json, "\"operationId\":\"")
                .ok_or_else(|| crate::Error::Decode("missing operationId".into()))?,
        }),
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
            event_queue: VecDeque::new(),
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

        let hex: String = result_as_str(&result)
            .map(Into::into)
            .ok_or_else(|| crate::Error::Decode("header response not a string".into()))?;

        decode_header(&hex)
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
        let hashes = core::mem::take(&mut self.pending_unpin);
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

    /// Poll the subscription until we get a result for the given operation,
    /// processing other follow events as side effects.
    async fn wait_for_operation(&mut self, target: &str) -> crate::Result<OperationResult> {
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
        let mut items_json = String::from("[");
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                items_json.push(',');
            }
            items_json.push_str(&format!(r#"{{"key":"{}","type":"value"}}"#, k));
        }
        items_json.push(']');

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
            OperationStarted::Started { operation_id } => {
                match self.wait_for_operation(operation_id).await? {
                    OperationResult::StorageItems(items) => Ok(items),
                    OperationResult::Error(e) => Err(crate::Error::Node(e)),
                    _ => Err(crate::Error::Node("unexpected result".into())),
                }
            }
            OperationStarted::LimitReached => Err(crate::Error::Node(
                "chainHead operation limit reached".into(),
            )),
        }
    }

    /// Query storage using descendantsValues type.
    async fn storage_descendants(&mut self, prefix: &str) -> crate::Result<Vec<StorageItem>> {
        let hash = self.prepare_operation().await?;
        let items_json = format!(r#"[{{"key":"{}","type":"descendantsValues"}}]"#, prefix);

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
            OperationStarted::Started { operation_id } => {
                match self.wait_for_operation(operation_id).await? {
                    OperationResult::StorageItems(items) => Ok(items),
                    OperationResult::Error(e) => Err(crate::Error::Node(e)),
                    _ => Err(crate::Error::Node("unexpected result".into())),
                }
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
            OperationStarted::Started { operation_id } => {
                match self.wait_for_operation(operation_id).await? {
                    OperationResult::CallDone(hex_output) => {
                        hex::decode(hex_output.trim_start_matches("0x"))
                            .map_err(|_| crate::Error::Decode("runtime call hex decode".into()))
                    }
                    OperationResult::Error(e) => Err(crate::Error::Node(e)),
                    _ => Err(crate::Error::Node("unexpected result".into())),
                }
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

    async fn watch_transaction(
        &mut self,
        ext: &[u8],
        wait_for: crate::WaitFor,
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

        let mut receipt = crate::TransactionReceipt::default();

        loop {
            let (event_sub_id, event_json) = self
                .rpc
                .next_event()
                .await
                .ok_or(crate::Error::SubscriptionClosed)?;

            if event_sub_id == sub_id {
                let event = parse_tx_event(&event_json)
                    .map_err(|e| crate::Error::Decode(format!("tx event: {e}")))?;
                match event {
                    TxEvent::BestChainBlockIncluded { block: Some(block) } => {
                        receipt.best_block_hash = Some(block.hash);
                        receipt.extrinsic_index = Some(block.index);
                        if matches!(wait_for, crate::WaitFor::BestBlock) {
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
                        return Ok(receipt);
                    }
                    TxEvent::Invalid { error } => {
                        return Err(crate::Error::OperationFailed(format!(
                            "tx invalid: {error}"
                        )));
                    }
                    TxEvent::Dropped { error } => {
                        return Err(crate::Error::OperationFailed(format!(
                            "tx dropped: {error}"
                        )));
                    }
                    TxEvent::Error { error } => {
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

    async fn submit(&mut self, ext: &[u8], wait_for_finalization: bool) -> crate::Result<()> {
        self.watch_transaction(
            ext,
            if wait_for_finalization {
                crate::WaitFor::Finalized
            } else {
                crate::WaitFor::BestBlock
            },
        )
        .await
        .map(|_| ())
    }

    async fn submit_transaction(
        &mut self,
        ext: &crate::EncodedExtrinsic,
        wait_for: crate::WaitFor,
    ) -> crate::Result<crate::TransactionReceipt> {
        self.watch_transaction(&ext.bytes, wait_for).await
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

    async fn block_info(&mut self, at: Option<u32>) -> crate::Result<meta::BlockInfo> {
        match at {
            Some(0) => Ok(meta::BlockInfo {
                number: 0,
                hash: self.genesis_hash,
                parent: self.genesis_hash,
            }),
            None => {
                let finalized_hash = self.finalized_hash.clone();
                let header = self.header(&finalized_hash).await?;
                let mut h = [0u8; 32];
                hex::decode_to_slice(finalized_hash.trim_start_matches("0x"), &mut h)
                    .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;
                let mut parent = [0u8; 32];
                hex::decode_to_slice(header.parent_hash.trim_start_matches("0x"), &mut parent)
                    .map_err(|_| crate::Error::Decode("parent hash hex decode failed".into()))?;
                Ok(meta::BlockInfo {
                    number: header.number,
                    hash: h,
                    parent,
                })
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
