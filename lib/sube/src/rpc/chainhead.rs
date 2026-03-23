//! ChainHead v1 session manager.
//!
//! Manages a `chainHead_v1_follow` subscription and provides high-level
//! methods for storage queries, runtime calls, and block tracking.
//!
//! Pin strategy: we only keep the latest finalized block pinned.
//! Between operations we drain queued events and unpin everything
//! except the current finalized block.

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use codec::Decode;
use serde::Deserialize;

use super::{to_hex, Rpc, RpcSubscription};
use crate::meta::{self, Metadata};
use crate::prelude::*;

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

#[derive(Deserialize, Debug)]
#[serde(tag = "event")]
enum FollowEvent {
    #[serde(rename = "initialized")]
    Initialized {
        #[serde(rename = "finalizedBlockHashes")]
        finalized_block_hashes: Vec<String>,
        #[serde(rename = "finalizedBlockRuntime")]
        _finalized_block_runtime: Option<serde_json::Value>,
    },
    #[serde(rename = "newBlock")]
    NewBlock {
        #[serde(rename = "blockHash")]
        block_hash: String,
        #[serde(rename = "parentBlockHash")]
        _parent_block_hash: String,
        #[serde(rename = "newRuntime")]
        _new_runtime: Option<serde_json::Value>,
    },
    #[serde(rename = "bestBlockChanged")]
    BestBlockChanged {
        #[serde(rename = "bestBlockHash")]
        _best_block_hash: String,
    },
    #[serde(rename = "finalized")]
    Finalized {
        #[serde(rename = "finalizedBlockHashes")]
        finalized_block_hashes: Vec<String>,
        #[serde(rename = "prunedBlockHashes")]
        pruned_block_hashes: Vec<String>,
    },
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "operationCallDone")]
    OperationCallDone {
        #[serde(rename = "operationId")]
        operation_id: String,
        output: String,
    },
    #[serde(rename = "operationStorageItems")]
    OperationStorageItems {
        #[serde(rename = "operationId")]
        operation_id: String,
        items: Vec<OperationStorageItemJson>,
    },
    #[serde(rename = "operationStorageDone")]
    OperationStorageDone {
        #[serde(rename = "operationId")]
        operation_id: String,
    },
    #[serde(rename = "operationError")]
    OperationError {
        #[serde(rename = "operationId")]
        operation_id: String,
        error: String,
    },
    #[serde(rename = "operationInaccessible")]
    OperationInaccessible {
        #[serde(rename = "operationId")]
        operation_id: String,
    },
    #[serde(rename = "operationWaitingForContinue")]
    OperationWaitingForContinue {
        #[serde(rename = "operationId")]
        operation_id: String,
    },
}

#[derive(Deserialize, Debug, Clone)]
struct OperationStorageItemJson {
    key: String,
    value: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "result")]
enum OperationStarted {
    #[serde(rename = "started")]
    Started {
        #[serde(rename = "operationId")]
        operation_id: String,
    },
    #[serde(rename = "limitReached")]
    LimitReached,
}

// --- Archive storage event types ---

#[derive(Deserialize, Debug)]
#[serde(tag = "event")]
enum ArchiveStorageEvent {
    #[serde(rename = "items")]
    Items { items: Vec<ArchiveStorageItemJson> },
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    Error { error: String },
    #[serde(rename = "waitingForContinue")]
    WaitingForContinue,
}

#[derive(Deserialize, Debug)]
struct ArchiveStorageItemJson {
    key: String,
    value: Option<String>,
}

// --- Transaction watch event types ---

#[derive(Deserialize, Debug)]
#[serde(tag = "event")]
#[allow(dead_code)]
enum TxEvent {
    #[serde(rename = "validated")]
    Validated,
    #[serde(rename = "broadcasted")]
    Broadcasted {
        #[serde(rename = "numPeers")]
        _num_peers: u32,
    },
    #[serde(rename = "bestChainBlockIncluded")]
    BestChainBlockIncluded { block: Option<TxEventBlock> },
    #[serde(rename = "finalized")]
    Finalized { block: TxEventBlock },
    #[serde(rename = "invalid")]
    Invalid { error: String },
    #[serde(rename = "dropped")]
    Dropped {
        #[serde(default)]
        error: String,
    },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct TxEventBlock {
    hash: String,
    index: u32,
}

impl<R: Rpc + RpcSubscription> ChainHead<R> {
    /// Create a new ChainHead session. Fetches genesis hash and starts follow subscription.
    pub async fn new(mut rpc: R) -> crate::Result<Self> {
        let genesis_hex: String = serde_json::from_value(
            rpc.rpc("chainSpec_v1_genesisHash", serde_json::json!([]))
                .await
                .map_err(|e| crate::Error::Node(e.to_string()))?,
        )
        .map_err(|e| crate::Error::Decode(e.to_string()))?;

        let mut genesis_hash = [0u8; 32];
        hex::decode_to_slice(genesis_hex.trim_start_matches("0x"), &mut genesis_hash)
            .map_err(|_| crate::Error::Decode("genesis hash hex decode failed".into()))?;

        let follow_sub_id = rpc
            .subscribe("chainHead_v1_follow", serde_json::json!([true]))
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
        };

        ch.wait_initialized().await?;

        Ok(ch)
    }

    /// Poll the subscription until the initialized event arrives.
    async fn wait_initialized(&mut self) -> crate::Result<()> {
        loop {
            let (_, event_json) = self.next_follow_event().await?;
            let event: FollowEvent = serde_json::from_value(event_json)
                .map_err(|e| crate::Error::Decode(format!("follow event: {e}")))?;

            if let FollowEvent::Initialized {
                finalized_block_hashes,
                ..
            } = event
            {
                if let Some(h) = finalized_block_hashes.last() {
                    self.finalized_hash = h.clone();
                }
                // Queue unpin for all initialized blocks except the latest finalized
                for h in finalized_block_hashes.iter().rev().skip(1) {
                    self.pending_unpin.push(h.clone());
                }
                return Ok(());
            }
        }
    }

    /// Wait for the next event on the follow subscription, buffering events
    /// from other subscriptions for later retrieval.
    async fn next_follow_event(&mut self) -> crate::Result<(String, serde_json::Value)> {
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
    /// This is used when we receive an event from a non-follow subscription.
    fn rpc_rebuffer(&mut self, _sub_id: String, _value: serde_json::Value) {
        // Events from non-follow subscriptions are currently discarded.
        // Archive operations use wait_for_archive_event which handles routing.
        log::trace!("discarding event from non-follow subscription");
    }

    /// Flush all pending unpins in a single batch RPC call.
    async fn flush_unpins(&mut self) {
        let hashes = core::mem::take(&mut self.pending_unpin);
        if hashes.is_empty() {
            return;
        }
        let _ = self
            .rpc
            .rpc(
                "chainHead_v1_unpin",
                serde_json::json!([&self.follow_sub_id, hashes]),
            )
            .await;
    }

    /// Record a lifecycle event. Only updates state and queues unpins — never
    /// actually calls unpin (that happens in flush_unpins/prepare_operation).
    fn record_lifecycle_event(&mut self, event: FollowEvent) {
        match event {
            FollowEvent::Initialized {
                finalized_block_hashes,
                ..
            } => {
                if let Some(h) = finalized_block_hashes.last() {
                    self.finalized_hash = h.clone();
                }
            }
            FollowEvent::NewBlock { block_hash, .. } => {
                self.pending_unpin.push(block_hash);
            }
            FollowEvent::Finalized {
                finalized_block_hashes,
                pruned_block_hashes,
            } => {
                let old = core::mem::take(&mut self.finalized_hash);

                if !old.is_empty() {
                    self.pending_unpin.push(old);
                }
                for h in pruned_block_hashes {
                    self.pending_unpin.push(h);
                }
                for h in finalized_block_hashes.iter().rev().skip(1) {
                    self.pending_unpin.push(h.clone());
                }

                if let Some(new) = finalized_block_hashes.last() {
                    self.finalized_hash = new.clone();
                    let was_pending = self.pending_unpin.iter().any(|h| h == new);
                    self.pending_unpin.retain(|h| h != new);
                    if !was_pending {
                        self.needs_refollow = true;
                    }
                }
            }
            FollowEvent::BestBlockChanged { .. } => {}
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
            let event: FollowEvent = match serde_json::from_value(event_json) {
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
            self.follow_sub_id = self
                .rpc
                .subscribe("chainHead_v1_follow", serde_json::json!([true]))
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

            let event: FollowEvent = serde_json::from_value(event_json)
                .map_err(|e| crate::Error::Decode(format!("follow event: {e}")))?;

            match event {
                FollowEvent::Stop => {
                    return Err(crate::Error::SubscriptionClosed);
                }
                FollowEvent::OperationStorageItems {
                    operation_id,
                    items,
                } => {
                    let entry = self.storage_accum.entry(operation_id).or_default();
                    for item in items {
                        entry.push(StorageItem {
                            key: item.key,
                            value: item.value,
                        });
                    }
                }
                FollowEvent::OperationStorageDone { operation_id } => {
                    let items = self.storage_accum.remove(&operation_id).unwrap_or_default();
                    if operation_id == target {
                        return Ok(OperationResult::StorageItems(items));
                    }
                }
                FollowEvent::OperationCallDone {
                    operation_id,
                    output,
                } => {
                    if operation_id == target {
                        return Ok(OperationResult::CallDone(output));
                    }
                }
                FollowEvent::OperationError {
                    operation_id,
                    error,
                } => {
                    if operation_id == target {
                        return Ok(OperationResult::Error(error));
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
                                serde_json::json!([&self.follow_sub_id, &operation_id]),
                            )
                            .await;
                    }
                }
                other => self.record_lifecycle_event(other),
            }
        }
    }

    /// Send a storage query and wait for results.
    async fn storage(&mut self, keys: &[String]) -> crate::Result<Vec<StorageItem>> {
        let hash = self.prepare_operation().await?;
        let items: Vec<serde_json::Value> = keys
            .iter()
            .map(|k| serde_json::json!({"key": k, "type": "value"}))
            .collect();

        let result = self
            .rpc
            .rpc(
                "chainHead_v1_storage",
                serde_json::json!([&self.follow_sub_id, &hash, items, null]),
            )
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        let started: OperationStarted = serde_json::from_value(result)
            .map_err(|e| crate::Error::Node(format!("bad storage response: {e}")))?;

        match started {
            OperationStarted::Started { operation_id } => {
                match self.wait_for_operation(&operation_id).await? {
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
        let items = serde_json::json!([{"key": prefix, "type": "descendantsValues"}]);

        let result = self
            .rpc
            .rpc(
                "chainHead_v1_storage",
                serde_json::json!([&self.follow_sub_id, &hash, items, null]),
            )
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        let started: OperationStarted = serde_json::from_value(result)
            .map_err(|e| crate::Error::Node(format!("bad storage response: {e}")))?;

        match started {
            OperationStarted::Started { operation_id } => {
                match self.wait_for_operation(&operation_id).await? {
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

    /// Execute a runtime call at a pinned block.
    async fn runtime_call(&mut self, function: &str, call_data: &str) -> crate::Result<Vec<u8>> {
        let hash = self.prepare_operation().await?;
        let result = self
            .rpc
            .rpc(
                "chainHead_v1_call",
                serde_json::json!([&self.follow_sub_id, &hash, function, call_data]),
            )
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        let started: OperationStarted = serde_json::from_value(result)
            .map_err(|e| crate::Error::Node(format!("bad call response: {e}")))?;

        match started {
            OperationStarted::Started { operation_id } => {
                match self.wait_for_operation(&operation_id).await? {
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
            .rpc("archive_v1_hashByHeight", serde_json::json!([height]))
            .await
            .map_err(|e| crate::Error::Node(format!("archive_v1_hashByHeight: {e}")))?;

        // Returns an array of hashes (usually one for canonical chain)
        let hashes: Vec<String> = serde_json::from_value(result)
            .map_err(|e| crate::Error::Decode(format!("hashByHeight response: {e}")))?;

        hashes
            .into_iter()
            .next()
            .ok_or(crate::Error::BadBlockNumber)
    }

    /// Query storage at a historical block via `archive_v1_storage` (subscription-based).
    async fn archive_storage(
        &mut self,
        block_hash: &str,
        keys: &[String],
    ) -> crate::Result<Vec<StorageItem>> {
        let items: Vec<serde_json::Value> = keys
            .iter()
            .map(|k| serde_json::json!({"key": k, "type": "value"}))
            .collect();

        let archive_sub_id = self
            .rpc
            .subscribe("archive_v1_storage", serde_json::json!([block_hash, items]))
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
                let event: ArchiveStorageEvent = serde_json::from_value(event_json)
                    .map_err(|e| crate::Error::Decode(format!("archive event: {e}")))?;

                match event {
                    ArchiveStorageEvent::Items { items } => {
                        for item in items {
                            result_items.push(StorageItem {
                                key: item.key,
                                value: item.value,
                            });
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
                                serde_json::json!([archive_sub_id]),
                            )
                            .await;
                    }
                }
            } else if sub_id == self.follow_sub_id {
                // Process follow events that arrive while waiting for archive results
                if let Ok(event) = serde_json::from_value::<FollowEvent>(event_json) {
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

        let mut result = Vec::new();
        for key in &keys {
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

    async fn get_keys_paged(
        &mut self,
        from: crate::RawKey,
        _size: u16,
        _to: Option<crate::RawKey>,
    ) -> crate::Result<Vec<crate::RawKey>> {
        let prefix = to_hex(&from);

        let items = self.storage_descendants(&prefix).await?;

        items
            .into_iter()
            .map(|item| {
                hex::decode(item.key.trim_start_matches("0x"))
                    .map_err(|_| crate::Error::Decode("hex decode failed".into()))
            })
            .collect()
    }

    async fn submit(&mut self, ext: &[u8]) -> crate::Result<()> {
        let hex = to_hex(ext);

        let sub_id = self
            .rpc
            .subscribe(
                "transactionWatch_v1_submitAndWatch",
                serde_json::json!([hex]),
            )
            .await
            .map_err(|e| crate::Error::Node(format!("tx watch: {e}")))?;

        // Wait for finalization or terminal error
        loop {
            let (event_sub_id, event_json) = self
                .rpc
                .next_event()
                .await
                .ok_or(crate::Error::SubscriptionClosed)?;

            if event_sub_id == sub_id {
                let event: TxEvent = serde_json::from_value(event_json)
                    .map_err(|e| crate::Error::Decode(format!("tx event: {e}")))?;

                match event {
                    TxEvent::Finalized { .. } => return Ok(()),
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
                    // Validated, Broadcasted, BestChainBlockIncluded — keep waiting
                    _ => {}
                }
            } else if event_sub_id == self.follow_sub_id {
                if let Ok(event) = serde_json::from_value::<FollowEvent>(event_json) {
                    match event {
                        FollowEvent::Stop => self.needs_refollow = true,
                        other => self.record_lifecycle_event(other),
                    }
                }
            }
        }
    }

    async fn metadata(&mut self) -> crate::Result<Metadata> {
        let raw = self.runtime_call("Metadata_metadata", "0x").await?;

        let mut cursor = raw.as_slice();
        let _len = <codec::Compact<u32>>::decode(&mut cursor)
            .map_err(|_| crate::Error::Decode("compact prefix".into()))?;

        meta::from_bytes(&mut cursor).map_err(|_| crate::Error::BadMetadata)
    }

    async fn block_info(&mut self, at: Option<u32>) -> crate::Result<meta::BlockInfo> {
        match at {
            Some(0) => Ok(meta::BlockInfo {
                number: 0,
                hash: self.genesis_hash,
                parent: self.genesis_hash,
            }),
            None => {
                let mut h = [0u8; 32];
                hex::decode_to_slice(self.finalized_hash.trim_start_matches("0x"), &mut h)
                    .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;
                Ok(meta::BlockInfo {
                    number: 0,
                    hash: h,
                    parent: h,
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
