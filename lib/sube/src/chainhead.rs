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
use no_std_async::Mutex;
use serde::Deserialize;

use crate::meta::{self, Metadata};
use crate::prelude::*;
use crate::rpc::{to_hex, Rpc, RpcSubscription, Subscription};

/// Mutable session state, behind a Mutex for interior mutability
/// (Backend trait methods take `&self`).
struct Inner<R> {
    rpc: R,
    sub: Subscription,
    follow_sub_id: String,
    finalized_hash: String,
    /// Accumulates storage items for in-progress operations.
    storage_accum: BTreeMap<String, Vec<StorageItem>>,
    /// Block hashes that are pinned on the server but we don't need.
    /// Unpinned lazily in bulk before each operation.
    pending_unpin: Vec<String>,
    /// Set when a finalized event arrives after flush — means finalized_hash
    /// points to an already-unpinned block and we need to refollow.
    needs_refollow: bool,
}

/// A chainHead session that manages a `chainHead_v1_follow` subscription.
pub(crate) struct ChainHead<R> {
    inner: Mutex<Inner<R>>,
    genesis_hash: [u8; 32],
}

/// Result from a chainHead operation.
enum OperationResult {
    StorageItems(Vec<StorageItem>),
    CallDone(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) struct StorageItem {
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

impl<R: Rpc + RpcSubscription> ChainHead<R> {
    /// Create a new ChainHead session. Fetches genesis hash and starts follow subscription.
    pub async fn new(rpc: R) -> crate::Result<Self> {
        let genesis_hex: String = serde_json::from_value(
            rpc.rpc("chainSpec_v1_genesisHash", serde_json::json!([]))
                .await
                .map_err(|e| crate::Error::Node(e.to_string()))?,
        )
        .map_err(|e| crate::Error::Decode(e.to_string()))?;

        let mut genesis_hash = [0u8; 32];
        hex::decode_to_slice(genesis_hex.trim_start_matches("0x"), &mut genesis_hash)
            .map_err(|_| crate::Error::Decode("genesis hash hex decode failed".into()))?;

        let (sub_id, sub) = rpc
            .subscribe("chainHead_v1_follow", serde_json::json!([true]))
            .await
            .map_err(|e| crate::Error::Node(format!("follow subscribe failed: {e}")))?;

        let mut inner = Inner {
            rpc,
            sub,
            follow_sub_id: sub_id,
            finalized_hash: String::new(),
            storage_accum: BTreeMap::new(),
            pending_unpin: Vec::new(),
            needs_refollow: false,
        };

        // Wait for initialized event
        inner.wait_initialized().await?;

        Ok(ChainHead {
            inner: Mutex::new(inner),
            genesis_hash,
        })
    }
}

impl<R: Rpc + RpcSubscription> Inner<R> {
    /// Poll the subscription until the initialized event arrives.
    async fn wait_initialized(&mut self) -> crate::Result<()> {
        loop {
            let event_json = self.sub.next().await.ok_or(crate::Error::Node(
                "subscription closed before receiving initialized event".into(),
            ))?;
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
                // Queue for unpinning — we never query non-finalized blocks.
                self.pending_unpin.push(block_hash);
            }
            FollowEvent::Finalized {
                finalized_block_hashes,
                pruned_block_hashes,
            } => {
                let old = core::mem::take(&mut self.finalized_hash);

                // Queue old finalized for unpin
                if !old.is_empty() {
                    self.pending_unpin.push(old);
                }
                // Queue pruned blocks
                for h in pruned_block_hashes {
                    self.pending_unpin.push(h);
                }
                // Queue all finalized except latest
                for h in finalized_block_hashes.iter().rev().skip(1) {
                    self.pending_unpin.push(h.clone());
                }

                // The latest finalized block: if it's still in pending_unpin
                // (not yet flushed), remove it so it stays pinned.
                // If it's NOT in pending_unpin, it was already unpinned in a
                // previous flush — we need to refollow to get a fresh pin.
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

        while let Some(event_json) = self.sub.try_next() {
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

        // Flush accumulated unpins
        self.flush_unpins().await;

        if stopped || self.needs_refollow {
            // Re-subscribe to get a fresh session with a pinned finalized block
            self.needs_refollow = false;
            let (sub_id, sub) = self
                .rpc
                .subscribe("chainHead_v1_follow", serde_json::json!([true]))
                .await
                .map_err(|e| crate::Error::Node(format!("refollow failed: {e}")))?;
            self.sub = sub;
            self.follow_sub_id = sub_id;
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
            let event_json = self
                .sub
                .next()
                .await
                .ok_or(crate::Error::SubscriptionClosed)?;
            let event: FollowEvent = serde_json::from_value(event_json)
                .map_err(|e| crate::Error::Decode(format!("follow event: {e}")))?;

            match event {
                FollowEvent::Stop => {
                    return Err(crate::Error::SubscriptionClosed);
                }

                // --- Operation events ---
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

                // Lifecycle events — just record, don't unpin mid-operation.
                // Unpins happen in prepare_operation before the next op.
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
}

// --- Backend implementation ---

impl<R: Rpc + RpcSubscription> crate::Backend for ChainHead<R> {
    async fn get_storage_items(
        &self,
        keys: Vec<crate::RawKey>,
        block: Option<u32>,
    ) -> crate::Result<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        if block.is_some() {
            return Err(crate::Error::BadBlockNumber);
        }
        let mut inner = self.inner.lock().await;
        let hex_keys: Vec<String> = keys.iter().map(|k| to_hex(k)).collect();

        let items = inner.storage(&hex_keys).await?;

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
        &self,
        from: crate::RawKey,
        _size: u16,
        _to: Option<crate::RawKey>,
    ) -> crate::Result<Vec<crate::RawKey>> {
        let mut inner = self.inner.lock().await;
        let prefix = to_hex(&from);

        let items = inner.storage_descendants(&prefix).await?;

        items
            .into_iter()
            .map(|item| {
                hex::decode(item.key.trim_start_matches("0x"))
                    .map_err(|_| crate::Error::Decode("hex decode failed".into()))
            })
            .collect()
    }

    async fn submit(&self, ext: &[u8]) -> crate::Result<()> {
        let inner = self.inner.lock().await;
        let hex = to_hex(ext);
        inner
            .rpc
            .rpc("transaction_v1_broadcast", serde_json::json!([hex]))
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;
        Ok(())
    }

    async fn metadata(&self) -> crate::Result<Metadata> {
        let mut inner = self.inner.lock().await;
        let raw = inner.runtime_call("Metadata_metadata", "0x").await?;

        // Metadata_metadata returns OpaqueMetadata (SCALE Vec<u8>), always has compact length prefix
        let mut cursor = raw.as_slice();
        let _len = <codec::Compact<u32>>::decode(&mut cursor)
            .map_err(|_| crate::Error::Decode("compact prefix".into()))?;

        meta::from_bytes(&mut cursor).map_err(|_| crate::Error::BadMetadata)
    }

    async fn block_info(&self, at: Option<u32>) -> crate::Result<meta::BlockInfo> {
        match at {
            Some(0) => Ok(meta::BlockInfo {
                number: 0,
                hash: self.genesis_hash,
                parent: self.genesis_hash,
            }),
            None => {
                let inner = self.inner.lock().await;
                let mut h = [0u8; 32];
                hex::decode_to_slice(inner.finalized_hash.trim_start_matches("0x"), &mut h)
                    .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;
                Ok(meta::BlockInfo {
                    number: 0,
                    hash: h,
                    parent: h,
                })
            }
            Some(_) => Err(crate::Error::BadBlockNumber),
        }
    }
}
