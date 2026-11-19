//! A JSON-RPC session over a shared [`Fork`](crate::Fork).
//!
//! [`ForkRpc`] speaks the new JSON-RPC API (`chainHead_v1`, `transaction_v1`,
//! `archive_v1`...) plus `dev_*` methods, with requests and events as raw
//! JSON strings, the way smoldot's light client does. sube's `fork` feature
//! serves it to `ChainHead`. Each session has its own subscriptions and event
//! queue, while the chain is shared as a [`SharedFork`](crate::SharedFork): code holding
//! the handle can build blocks, write storage or time-travel at any time.
//!
//! Every block is final at once, so a follow subscription gets `newBlock`,
//! `bestBlockChanged` and `finalized` together. Events are produced from
//! per-subscription cursors when the session is polled, and a waiting
//! `next_event` is woken when the fork builds a block, so any task on the same
//! executor can move the chain forward.

use std::collections::{BTreeMap, VecDeque};
use std::task::Poll;

use serde_json::{Value, json};

use crate::executor::RuntimeInfo;
use crate::util::{blake2_256, hex0x, unhex};
use crate::{Diff, Error, Hash, SharedFork};

const INVALID_PARAMS: i64 = -32602;
const METHOD_NOT_FOUND: i64 = -32601;
const FORK_ERROR: i64 = -32000;
/// The block is unknown or its state is pruned.
const INVALID_BLOCK: i64 = -32801;

const METHODS: &[&str] = &[
    "chainSpec_v1_genesisHash",
    "chainSpec_v1_chainName",
    "chainSpec_v1_properties",
    "chainHead_v1_follow",
    "chainHead_v1_unfollow",
    "chainHead_v1_header",
    "chainHead_v1_body",
    "chainHead_v1_call",
    "chainHead_v1_storage",
    "chainHead_v1_continue",
    "chainHead_v1_stopOperation",
    "chainHead_v1_unpin",
    "transactionWatch_v1_submitAndWatch",
    "transactionWatch_v1_unwatch",
    "transaction_v1_broadcast",
    "transaction_v1_stop",
    "archive_v1_genesisHash",
    "archive_v1_finalizedHeight",
    "archive_v1_hashByHeight",
    "archive_v1_header",
    "archive_v1_call",
    "archive_v1_storage",
    "system_chain",
    "system_name",
    "system_version",
    "system_properties",
    "rpc_methods",
    "dev_newBlock",
    "dev_setStorage",
    "dev_timeTravel",
];

struct Follow {
    /// Index of the last block announced.
    announced: usize,
    with_runtime: bool,
}

struct Watch {
    hash: Hash,
    /// Index of the last block searched for the transaction.
    checked: usize,
}

pub struct ForkRpc {
    fork: SharedFork,
    next_id: u64,
    follows: BTreeMap<String, Follow>,
    watches: BTreeMap<String, Watch>,
    /// How much of [`Fork::rejected`](crate::Fork::rejected) watchers were told about.
    rejected_seen: usize,
    queue: VecDeque<(String, String)>,
}

/// A JSON-RPC error response.
#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

type RpcResult<T> = Result<T, RpcError>;

fn err(code: i64, message: impl Into<String>) -> RpcError {
    RpcError {
        code,
        message: message.into(),
    }
}

fn fork_err(e: Error) -> RpcError {
    match e {
        Error::UnknownBlock => err(INVALID_BLOCK, e.to_string()),
        e => err(FORK_ERROR, e.to_string()),
    }
}

fn param_str(p: &Value, i: usize) -> RpcResult<&str> {
    p.get(i)
        .and_then(Value::as_str)
        .ok_or_else(|| err(INVALID_PARAMS, format!("param {i} must be a string")))
}

fn param_hex(p: &Value, i: usize) -> RpcResult<Vec<u8>> {
    unhex(param_str(p, i)?).ok_or_else(|| err(INVALID_PARAMS, format!("param {i} must be hex")))
}

fn param_u64(p: &Value, i: usize) -> RpcResult<u64> {
    p.get(i)
        .and_then(Value::as_u64)
        .ok_or_else(|| err(INVALID_PARAMS, format!("param {i} must be a number")))
}

/// The `RuntimeEvent` of the new JSON-RPC API.
fn runtime_json(info: &RuntimeInfo) -> Value {
    let apis: serde_json::Map<String, Value> = info
        .apis
        .iter()
        .map(|(name, v)| (hex0x(name), json!(v)))
        .collect();
    json!({
        "type": "valid",
        "spec": {
            "specName": info.spec_name,
            "implName": info.impl_name,
            "specVersion": info.spec_version,
            "implVersion": info.impl_version,
            "transactionVersion": info.transaction_version,
            "apis": apis,
        },
    })
}

fn started(op: &str) -> Value {
    json!({ "result": "started", "operationId": op })
}

impl ForkRpc {
    pub fn new(fork: SharedFork) -> Self {
        let rejected_seen = fork.borrow().rejected().len();
        ForkRpc {
            fork,
            next_id: 1,
            follows: BTreeMap::new(),
            watches: BTreeMap::new(),
            rejected_seen,
            queue: VecDeque::new(),
        }
    }

    /// The fork this session serves.
    pub fn fork(&self) -> &SharedFork {
        &self.fork
    }

    fn id(&mut self) -> String {
        self.next_id += 1;
        (self.next_id - 1).to_string()
    }

    fn push(&mut self, sub: &str, event: Value) {
        self.queue.push_back((sub.into(), event.to_string()));
    }

    /// Queue events for blocks built and transactions dropped since last time.
    fn sync(&mut self) {
        let ForkRpc {
            fork,
            follows,
            watches,
            rejected_seen,
            queue,
            ..
        } = self;
        let fork = fork.borrow();
        let head = fork.head_index();
        let mut push = |sub: &str, event: Value| queue.push_back((sub.into(), event.to_string()));

        for (sub, follow) in follows.iter_mut() {
            for block in &fork.blocks()[follow.announced + 1..=head] {
                let hash = hex0x(&block.hash);
                let new_runtime = block
                    .new_runtime
                    .as_ref()
                    .filter(|_| follow.with_runtime)
                    .map(runtime_json);
                push(
                    sub,
                    json!({ "event": "newBlock", "blockHash": hash, "parentBlockHash": hex0x(&block.parent), "newRuntime": new_runtime }),
                );
                push(
                    sub,
                    json!({ "event": "bestBlockChanged", "bestBlockHash": hash }),
                );
                push(
                    sub,
                    json!({ "event": "finalized", "finalizedBlockHashes": [hash], "prunedBlockHashes": [] }),
                );
            }
            follow.announced = head;
        }

        let rejected = &fork.rejected()[*rejected_seen..];
        watches.retain(|sub, watch| {
            for block in &fork.blocks()[watch.checked + 1..=head] {
                let index = block
                    .body
                    .iter()
                    .flatten()
                    .position(|x| blake2_256(x) == watch.hash);
                if let Some(index) = index {
                    let at = json!({ "hash": hex0x(&block.hash), "index": index });
                    push(
                        sub,
                        json!({ "event": "bestChainBlockIncluded", "block": at }),
                    );
                    push(sub, json!({ "event": "finalized", "block": at }));
                    return false;
                }
            }
            if let Some(r) = rejected.iter().find(|r| r.hash == watch.hash) {
                push(sub, json!({ "event": "invalid", "error": r.reason }));
                return false;
            }
            watch.checked = head;
            true
        });
        *rejected_seen = fork.rejected().len();
    }

    fn block_index(&self, p: &Value, i: usize) -> RpcResult<usize> {
        let hash: Hash = param_hex(p, i)?
            .try_into()
            .map_err(|_| err(INVALID_PARAMS, format!("param {i} must be a block hash")))?;
        self.fork
            .borrow()
            .index_of(&hash)
            .ok_or_else(|| fork_err(Error::UnknownBlock))
    }

    fn storage_items(
        &self,
        index: usize,
        items: &Value,
        child: Option<&[u8]>,
    ) -> RpcResult<Vec<Value>> {
        let fork = self.fork.borrow();
        let state = fork
            .view(index)
            .ok_or_else(|| fork_err(Error::UnknownBlock))?;
        let items = items
            .as_array()
            .ok_or_else(|| err(INVALID_PARAMS, "items must be an array"))?;
        let entry = |key: &[u8], value: &[u8], hashed: bool| match hashed {
            true => json!({ "key": hex0x(key), "hash": hex0x(&blake2_256(value)) }),
            false => json!({ "key": hex0x(key), "value": hex0x(value) }),
        };
        let mut out = Vec::new();
        for item in items {
            let key = item["key"]
                .as_str()
                .and_then(unhex)
                .ok_or_else(|| err(INVALID_PARAMS, "item key must be hex"))?;
            match item["type"].as_str().unwrap_or("value") {
                ty @ ("value" | "hash") => {
                    out.extend(state.get(child, &key).map(|v| entry(&key, v, ty == "hash")))
                }
                ty @ ("descendantsValues" | "descendantsHashes") => {
                    for k in state.keys_with_prefix(child, &key) {
                        out.extend(
                            state
                                .get(child, k)
                                .map(|v| entry(k, v, ty == "descendantsHashes")),
                        );
                    }
                }
                // Roots aren't authoritative on a fork (see the executor), so
                // there are no merkle values worth reporting.
                "closestDescendantMerkleValue" => {}
                other => {
                    return Err(err(
                        INVALID_PARAMS,
                        format!("unknown storage query type {other}"),
                    ));
                }
            }
        }
        Ok(out)
    }

    fn handle(&mut self, method: &str, p: &Value) -> RpcResult<Value> {
        match method.split('_').next() {
            Some("chainHead") => self.chain_head(method, p),
            Some("archive") => self.archive(method, p),
            Some("transaction" | "transactionWatch") => self.transaction(method, p),
            Some("dev") => self.dev(method, p),
            _ => self.node(method),
        }
    }

    fn node(&self, method: &str) -> RpcResult<Value> {
        let fork = self.fork.borrow();
        Ok(match method {
            "chainSpec_v1_genesisHash" => json!(hex0x(&fork.genesis_hash())),
            "chainSpec_v1_chainName" | "system_chain" => json!(fork.chain_name()),
            "chainSpec_v1_properties" | "system_properties" => {
                serde_json::from_str(fork.properties()).unwrap_or(json!({}))
            }
            "system_name" => json!("sube-fork"),
            "system_version" => json!(env!("CARGO_PKG_VERSION")),
            "rpc_methods" => json!({ "methods": METHODS }),
            other => return Err(err(METHOD_NOT_FOUND, format!("method not found: {other}"))),
        })
    }

    fn chain_head(&mut self, method: &str, p: &Value) -> RpcResult<Value> {
        let sub = param_str(p, 0)?.to_string();
        let result = match method {
            "chainHead_v1_header" => {
                let index = self.block_index(p, 1)?;
                json!(hex0x(&self.fork.borrow().blocks()[index].header))
            }
            "chainHead_v1_body" => {
                let index = self.block_index(p, 1)?;
                let op = self.id();
                let body = self.fork.borrow().blocks()[index]
                    .body
                    .as_ref()
                    .map(|b| b.iter().map(|x| hex0x(x)).collect::<Vec<_>>());
                let event = match body {
                    Some(body) => {
                        json!({ "event": "operationBodyDone", "operationId": op, "value": body })
                    }
                    None => json!({ "event": "operationInaccessible", "operationId": op }),
                };
                self.push(&sub, event);
                started(&op)
            }
            "chainHead_v1_call" => {
                let index = self.block_index(p, 1)?;
                let (function, data) = (param_str(p, 2)?.to_string(), param_hex(p, 3)?);
                let op = self.id();
                let event = match self.fork.borrow_mut().call(index, &function, &data) {
                    Ok(out) => {
                        json!({ "event": "operationCallDone", "operationId": op, "output": hex0x(&out) })
                    }
                    Err(e) => {
                        json!({ "event": "operationError", "operationId": op, "error": e.to_string() })
                    }
                };
                self.push(&sub, event);
                started(&op)
            }
            "chainHead_v1_storage" => {
                let index = self.block_index(p, 1)?;
                let child = p.get(3).and_then(Value::as_str).and_then(unhex);
                let items = self.storage_items(index, &p[2], child.as_deref())?;
                let op = self.id();
                if !items.is_empty() {
                    self.push(&sub, json!({ "event": "operationStorageItems", "operationId": op, "items": items }));
                }
                self.push(
                    &sub,
                    json!({ "event": "operationStorageDone", "operationId": op }),
                );
                started(&op)
            }
            "chainHead_v1_unfollow" => {
                self.follows.remove(&sub);
                Value::Null
            }
            // Everything is final and kept, and operations finish at once.
            "chainHead_v1_continue" | "chainHead_v1_stopOperation" | "chainHead_v1_unpin" => {
                Value::Null
            }
            other => return Err(err(METHOD_NOT_FOUND, format!("method not found: {other}"))),
        };
        Ok(result)
    }

    fn archive(&mut self, method: &str, p: &Value) -> RpcResult<Value> {
        let fork = self.fork.borrow();
        let result = match method {
            "archive_v1_genesisHash" => json!(hex0x(&fork.genesis_hash())),
            "archive_v1_finalizedHeight" => json!(fork.head().number),
            "archive_v1_hashByHeight" => {
                let block = u32::try_from(param_u64(p, 0)?)
                    .ok()
                    .and_then(|n| fork.index_of_number(n));
                json!(
                    block
                        .map(|i| hex0x(&fork.blocks()[i].hash))
                        .into_iter()
                        .collect::<Vec<_>>()
                )
            }
            "archive_v1_header" => {
                drop(fork);
                let index = self.block_index(p, 0)?;
                json!(hex0x(&self.fork.borrow().blocks()[index].header))
            }
            "archive_v1_call" => {
                drop(fork);
                let index = self.block_index(p, 0)?;
                let (function, data) = (param_str(p, 1)?.to_string(), param_hex(p, 2)?);
                match self.fork.borrow_mut().call(index, &function, &data) {
                    Ok(out) => json!({ "success": true, "value": hex0x(&out) }),
                    Err(e) => json!({ "success": false, "error": e.to_string() }),
                }
            }
            other => return Err(err(METHOD_NOT_FOUND, format!("method not found: {other}"))),
        };
        Ok(result)
    }

    fn transaction(&mut self, method: &str, p: &Value) -> RpcResult<Value> {
        match method {
            "transaction_v1_broadcast" => {
                let submitted = self.fork.borrow_mut().submit(param_hex(p, 0)?);
                match submitted {
                    Ok(_) => {
                        self.seal_pool()?;
                        Ok(json!(self.id()))
                    }
                    Err(e) => {
                        log::warn!("broadcast rejected: {e}");
                        Ok(Value::Null)
                    }
                }
            }
            "transactionWatch_v1_unwatch" => {
                self.watches.remove(param_str(p, 0)?);
                Ok(Value::Null)
            }
            "transaction_v1_stop" => Ok(Value::Null),
            other => Err(err(METHOD_NOT_FOUND, format!("method not found: {other}"))),
        }
    }

    fn dev(&mut self, method: &str, p: &Value) -> RpcResult<Value> {
        let mut fork = self.fork.borrow_mut();
        let result = match method {
            "dev_newBlock" => {
                let count = p
                    .get(0)
                    .and_then(|o| o["count"].as_u64())
                    .unwrap_or(1)
                    .max(1);
                let mut hash = [0; 32];
                for _ in 0..count {
                    hash = fork.new_block().map_err(fork_err)?.hash;
                }
                json!(hex0x(&hash))
            }
            // Either [[key, value|null], ...] or {"0xkey": "0xvalue"|null}.
            "dev_setStorage" => {
                let pairs: Vec<(&str, &Value)> = match p.get(0) {
                    Some(Value::Array(a)) => a
                        .iter()
                        .filter_map(|kv| Some((kv[0].as_str()?, &kv[1])))
                        .collect(),
                    Some(Value::Object(o)) => o.iter().map(|(k, v)| (k.as_str(), v)).collect(),
                    _ => return Err(err(INVALID_PARAMS, "changes must be an array or an object")),
                };
                let mut diff = Diff::default();
                for (key, value) in pairs {
                    let key = unhex(key)
                        .ok_or_else(|| err(INVALID_PARAMS, "storage keys must be hex"))?;
                    match value {
                        Value::Null => diff.remove(key),
                        v => diff.insert(
                            key,
                            v.as_str()
                                .and_then(unhex)
                                .ok_or_else(|| err(INVALID_PARAMS, "storage values must be hex"))?,
                        ),
                    }
                }
                fork.set_storage(diff).map_err(fork_err)?;
                json!(hex0x(&fork.head().hash))
            }
            "dev_timeTravel" => {
                let blocks = u32::try_from(param_u64(p, 0)?)
                    .map_err(|_| err(INVALID_PARAMS, "too many relay blocks"))?;
                fork.time_travel(blocks);
                json!(fork.next_relay_number())
            }
            other => return Err(err(METHOD_NOT_FOUND, format!("method not found: {other}"))),
        };
        Ok(result)
    }

    /// Instant seal ([`Fork::instant`](crate::Fork::instant)): build a block if the pool has
    /// anything valid.
    fn seal_pool(&mut self) -> RpcResult<()> {
        let mut fork = self.fork.borrow_mut();
        if fork.instant {
            fork.include_pool().map_err(fork_err)?;
        }
        Ok(())
    }

    fn subscribe_to(&mut self, method: &str, p: &Value) -> RpcResult<String> {
        let sub = self.id();
        match method {
            "chainHead_v1_follow" => {
                let with_runtime = p.get(0).and_then(Value::as_bool).unwrap_or(false);
                let fork = self.fork.borrow();
                let mut initialized = json!({ "event": "initialized", "finalizedBlockHashes": [hex0x(&fork.head().hash)] });
                if with_runtime {
                    initialized["finalizedBlockRuntime"] = runtime_json(fork.runtime());
                }
                let announced = fork.head_index();
                drop(fork);
                self.follows.insert(
                    sub.clone(),
                    Follow {
                        announced,
                        with_runtime,
                    },
                );
                self.push(&sub, initialized);
            }
            "transactionWatch_v1_submitAndWatch" => {
                let submitted = self.fork.borrow_mut().submit(param_hex(p, 0)?);
                match submitted {
                    Ok(hash) => {
                        let checked = self.fork.borrow().head_index();
                        self.push(&sub, json!({ "event": "validated" }));
                        self.watches.insert(sub.clone(), Watch { hash, checked });
                        if let Err(e) = self.seal_pool() {
                            self.watches.remove(&sub);
                            self.push(&sub, json!({ "event": "error", "error": e.message }));
                        }
                    }
                    Err(e) => {
                        self.push(&sub, json!({ "event": "invalid", "error": e.to_string() }))
                    }
                }
            }
            "archive_v1_storage" => {
                let index = self.block_index(p, 0)?;
                let child = p.get(2).and_then(Value::as_str).and_then(unhex);
                for mut item in self.storage_items(index, &p[1], child.as_deref())? {
                    item["event"] = json!("storage");
                    self.push(&sub, item);
                }
                self.push(&sub, json!({ "event": "storageDone" }));
            }
            other => {
                return Err(err(
                    METHOD_NOT_FOUND,
                    format!("subscription not found: {other}"),
                ));
            }
        }
        Ok(sub)
    }
}

fn parse_params(params: &str) -> Value {
    serde_json::from_str(params).unwrap_or_else(|_| json!([]))
}

/// The client-facing API: requests take JSON params and return the raw JSON
/// of the result; subscription events are `(subscription id, raw JSON)`.
impl ForkRpc {
    pub fn request(&mut self, method: &str, params: &str) -> RpcResult<String> {
        log::debug!("fork rpc {method} {params}");
        self.handle(method, &parse_params(params))
            .map(|v| v.to_string())
    }

    /// Start a subscription and return its id.
    pub fn subscribe(&mut self, method: &str, params: &str) -> RpcResult<String> {
        log::debug!("fork subscribe {method} {params}");
        self.subscribe_to(method, &parse_params(params))
    }

    /// Wait for the next event. Sessions never close, so this waits until
    /// a block is built if nothing is queued.
    pub async fn next_event(&mut self) -> (String, String) {
        std::future::poll_fn(|cx| {
            self.sync();
            match self.queue.pop_front() {
                Some(event) => Poll::Ready(event),
                None => {
                    self.fork.borrow_mut().register_waker(cx.waker());
                    Poll::Pending
                }
            }
        })
        .await
    }

    /// The next event if one is queued.
    pub fn try_next_event(&mut self) -> Option<(String, String)> {
        self.sync();
        self.queue.pop_front()
    }

    pub fn unsubscribe(&mut self, sub_id: &str) {
        self.follows.remove(sub_id);
        self.watches.remove(sub_id);
        self.queue.retain(|(sub, _)| sub != sub_id);
    }
}
