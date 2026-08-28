#![cfg_attr(not(feature = "std"), no_std)]
/*!
Lightweight Substrate client focused on size and portability.
Runs in `no_std` (including embedded Cortex-M), browser, and standard environments.

Uses runtime metadata (≥ v15) and [`scales`] to convert between SCALE binary
and human-readable formats (JSON, text) without hardcoded type information.

# Usage

```rust,ignore
# async fn example() -> sube::Result<()> {
let mut chain = sube::Sube::connect("wss://kreivo.io").await?;
let response = chain.query("system/number").await?;
assert!(response.to_text()?.is_some());
# Ok(())
# }
```

# Backends

| Feature | Description |
|---------|-------------|
| `ws` | Plain WebSocket via `async-tungstenite` + `smol` (std) |
| `wss` | WebSocket with Rustls/WebPKI TLS (implies `ws`) |
| `ws-edge` | WebSocket via `edge-ws` for embedded targets (no_std) |
| `ws-web` | Browser WebSocket via `gloo-net` (wasm32-unknown-unknown) |
| `smoldot-std` | Light client via `smoldot-light` (std, no external node) |

Browser apps target `wasm32-unknown-unknown` with `--features ws-web`. A full
in-browser light client (smoldot in wasm) is not yet available — upstream
`smoldot-light` only ships a std-only platform — so wasm apps should use
`ws-web` against a public RPC endpoint.

# Other Features

| Feature | Description |
|---------|-------------|
| `std` | Standard library support |
*/

#[macro_use]
extern crate alloc;

pub use alloc::rc::Rc;
pub use scales::{self, Registry, Value};
pub use value::DynValue;

pub use builder::{CallBuilder, FinalizedBlock, Sube, SubeBuilder};
#[cfg(feature = "ws-edge")]
pub use builder::{EdgeResources, EdgeSube, connect_edge, connect_edge_with_meta};
pub use extrinsic::{
    AssembledExtrinsic, AuthorizationSummary, ChainProperties, DispatchOutcome, EncodeCall,
    EncodedExtension, EncodedExtrinsic, ExternalSigningRequest, MortalEra, Mortality, PreparedCall,
    Text, TransactionEvent, TransactionOptions, TransactionReceipt, TransactionReport,
    TransactionValidity, TransactionWeight, WaitFor, mortal_era, mortality_expiry,
};
pub use meta::{BlockInfo, Metadata};
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
pub use rpc::chainhead::{BlockHeader, ChainEvent, ChainSession};
pub use signer::{Bytes, ExtrinsicAssembler, SignatureScheme, Signer, SignerFn};

use core::fmt;
use metadata::{self as meta, KeyValue, StorageKey};
use prelude::*;
use util::to_camel;

mod prelude {
    #[cfg(any(
        feature = "ws",
        feature = "ws-edge",
        feature = "smoldot-std",
        all(feature = "ws-web", target_arch = "wasm32")
    ))]
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

#[cfg(any(feature = "ws", feature = "smoldot-std"))]
pub mod backend;
pub mod builder;
pub mod extrinsic;
mod hasher;
#[cfg(feature = "libwallet")]
pub mod libwallet;
pub mod metadata;
pub mod rpc;
pub mod signer;
#[cfg(feature = "std")]
pub mod time;
pub mod util;
pub mod value;

/// Connect to a Substrate chain.
///
/// Returns a [`SubeBuilder`] — `.await` it to get a connected [`Sube`] handle.
///
/// ```rust,ignore
/// # async fn example() -> sube::Result<()> {
/// let response = sube::sube("wss://kreivo.io/system/number").await?;
/// assert!(response.to_text()?.is_some());
/// # Ok(())
/// # }
/// ```
pub fn sube(url: &str) -> SubeBuilder {
    SubeBuilder::new(url)
}

/// Default connection timeout (30 seconds).
pub const DEFAULT_TIMEOUT: core::time::Duration = core::time::Duration::from_secs(30);

pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
async fn query(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    path: &str,
    block: Option<u32>,
) -> Result<Response> {
    match resolve_query(meta, path)? {
        ResolvedQuery::Constant(entry) => Ok(Response::Value(entry, Rc::clone(meta))),
        ResolvedQuery::Storage(key) if !key.is_partial() => {
            let value = chain.get_storage_item(key.key(), block).await?;
            Ok(storage_response(value, key.ty, meta))
        }
        ResolvedQuery::Storage(key) => {
            let keys = chain.get_keys_paged(key.key(), 1000, None).await?;
            let values = chain.get_storage_items(keys, block).await?;
            partial_storage_response(values, &key, meta)
        }
    }
}

/// Query a constant or fully-keyed storage item at a known block hash.
///
/// This is the verifiable historical primitive for light clients: callers
/// obtain the hash from finalized chain state, then reuse it without asking an
/// untrusted peer to map a block height to a hash.
pub async fn query_at_hash(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    path: &str,
    block_hash: [u8; 32],
) -> Result<Response> {
    match resolve_query(meta, path)? {
        ResolvedQuery::Constant(entry) => Ok(Response::Value(entry, Rc::clone(meta))),
        ResolvedQuery::Storage(key) if !key.is_partial() => {
            let value = chain
                .get_storage_item_at_hash(key.key(), block_hash)
                .await?;
            Ok(storage_response(value, key.ty, meta))
        }
        ResolvedQuery::Storage(_) => Err(Error::BadInput),
    }
}

/// Query one bounded page of a partially-keyed storage map.
///
/// `start_key` is an opaque backend cursor. `block` selects the
/// finalized snapshot; when omitted, the backend captures its current
/// finalized block and returns it in [`StoragePage::at`].
pub async fn query_page(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    path: &str,
    limit: u16,
    start_key: Option<RawKey>,
    block: Option<u32>,
) -> Result<StoragePage> {
    if limit == 0 {
        return Err(Error::BadInput);
    }
    let ResolvedQuery::Storage(storage_key) = resolve_query(meta, path)? else {
        return Err(Error::BadInput);
    };
    if !storage_key.is_partial() {
        return Err(Error::BadInput);
    }
    ensure_decodable_map_keys(&storage_key)?;

    let at = chain.block_info(block).await?;
    let block_number = u32::try_from(at.number).map_err(|_| Error::BadBlockNumber)?;
    let requested = limit.checked_add(1).ok_or(Error::BadInput)?;
    let mut keys = chain
        .get_keys_paged_at(storage_key.key(), requested, start_key, Some(block_number))
        .await?;
    let has_more = keys.len() > usize::from(limit);
    keys.truncate(usize::from(limit));
    let next_key = has_more.then(|| keys.last().cloned()).flatten();
    let values = chain.get_storage_items(keys, Some(block_number)).await?;
    let entries = partial_storage_entries(values, &storage_key, meta)?;

    Ok(StoragePage {
        at,
        entries,
        next_key,
        metadata: Rc::clone(meta),
    })
}

/// Query one bounded map page at a finalized block hash already known to the
/// caller. Unlike the height-based archive path, this works with a light
/// client and preserves the exact snapshot across opaque cursor continuations.
pub async fn query_page_at_hash(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    path: &str,
    limit: u16,
    start_key: Option<RawKey>,
    at: BlockInfo,
) -> Result<StoragePage> {
    if limit == 0 {
        return Err(Error::BadInput);
    }
    let ResolvedQuery::Storage(storage_key) = resolve_query(meta, path)? else {
        return Err(Error::BadInput);
    };
    if !storage_key.is_partial() {
        return Err(Error::BadInput);
    }
    ensure_decodable_map_keys(&storage_key)?;

    let raw_page = chain
        .get_keys_page_at_hash(storage_key.key(), limit, start_key, at.hash)
        .await?;
    let values = chain
        .get_storage_items_at_hash(raw_page.keys, at.hash)
        .await?;
    let entries = partial_storage_entries(values, &storage_key, meta)?;

    Ok(StoragePage {
        at,
        entries,
        next_key: raw_page.next_cursor,
        metadata: Rc::clone(meta),
    })
}

pub(crate) enum ResolvedQuery {
    Constant(StorageEntry),
    Storage(StorageKey),
}

pub(crate) fn resolve_query(meta: &Metadata, path: &str) -> Result<ResolvedQuery> {
    let (pallet_name, item, mut keys) = parse_uri(path).ok_or(Error::BadInput)?;
    let pallet = meta
        .pallet_by_name(&pallet_name)
        .ok_or(Error::PalletNotFound(pallet_name))?;

    if item == "_constants" {
        let constant_name = keys.pop().ok_or(Error::MissingConstantName)?;
        let constant = pallet
            .constants
            .iter()
            .find(|constant| constant.name.eq_ignore_ascii_case(&constant_name))
            .ok_or_else(|| Error::ConstantNotFound(constant_name.clone()))?;
        return Ok(ResolvedQuery::Constant(StorageEntry::new(
            constant.value.clone(),
            constant.ty,
        )));
    }

    StorageKey::build_with_registry(&meta.registry, pallet, &item, &keys)
        .map(ResolvedQuery::Storage)
}

pub(crate) fn storage_response(
    value: Option<RawValue>,
    ty: scales::TypeId,
    meta: &Rc<Metadata>,
) -> Response {
    match value {
        Some(data) => Response::Value(StorageEntry::new(data, ty), Rc::clone(meta)),
        None => Response::None,
    }
}

#[cfg(test)]
fn partial_storage_response(
    values: Vec<(RawKey, Option<RawValue>)>,
    storage_key: &StorageKey,
    meta: &Rc<Metadata>,
) -> Result<Response> {
    let entries = partial_storage_entries(values, storage_key, meta)?
        .into_iter()
        .map(|entry| (entry.keys, entry.value))
        .collect();
    Ok(Response::ValueSet(entries, Rc::clone(meta)))
}

fn partial_storage_entries(
    values: Vec<(RawKey, Option<RawValue>)>,
    storage_key: &StorageKey,
    meta: &Rc<Metadata>,
) -> Result<Vec<StoragePageEntry>> {
    ensure_decodable_map_keys(storage_key)?;
    let prefix_len = storage_key.pallet.len() + storage_key.call.len();
    let mut decoded = Vec::with_capacity(values.len());

    for (raw_key, value) in values {
        let key = raw_key.get(prefix_len..).ok_or_else(|| {
            Error::Decode("storage key is shorter than its pallet/item prefix".into())
        })?;
        let mut offset = 0usize;
        let mut parts = Vec::with_capacity(storage_key.args.len());

        for (index, argument) in storage_key.args.iter().enumerate() {
            let type_id = match argument {
                KeyValue::Empty(ty) | KeyValue::Value((ty, _, _, _)) => *ty,
            };
            let hasher = storage_key
                .hashers
                .get(index)
                .ok_or_else(|| Error::Decode("storage key hasher mismatch".into()))?;
            offset = offset
                .checked_add(hasher.key_prefix_len())
                .ok_or_else(|| Error::Decode("storage key offset overflow".into()))?;
            let encoded = key
                .get(offset..)
                .ok_or_else(|| Error::Decode("truncated storage map key".into()))?;
            let entry = StorageEntry::new(encoded.to_vec(), type_id);
            let size = entry
                .as_value(&meta.registry)
                .size()
                .map_err(|error| Error::Decode(error.to_string()))?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| Error::Decode("storage key offset overflow".into()))?;
            let bytes = key
                .get(offset..end)
                .ok_or_else(|| Error::Decode("truncated storage map key".into()))?;
            parts.push(StorageEntry::new(bytes.to_vec(), type_id));
            offset = end;
        }

        if offset != key.len() {
            return Err(Error::Decode("storage map key has trailing bytes".into()));
        }
        decoded.push(StoragePageEntry {
            raw_key,
            keys: parts,
            value: value.map(|data| StorageEntry::new(data, storage_key.ty)),
        });
    }

    Ok(decoded)
}

fn ensure_decodable_map_keys(storage_key: &StorageKey) -> Result<()> {
    if storage_key
        .hashers
        .iter()
        .any(|hasher| !hasher.is_transparent())
    {
        return Err(Error::OperationFailed(
            "map keys cannot be decoded through a non-transparent storage hasher".into(),
        ));
    }
    Ok(())
}

pub(crate) fn parse_uri(uri: &str) -> Option<(String, String, Vec<String>)> {
    let mut path = uri.trim_matches('/').split('/');
    let pallet = path.next().map(to_camel)?;
    let item = path.next().map(to_camel)?;
    let map_keys = path.map(String::from).collect::<Vec<_>>();
    Some((pallet, item, map_keys))
}

// --- Public types ---

/// Owned raw SCALE-encoded data with its type id.
/// Call [`as_value`](StorageEntry::as_value) with a registry to decode.
#[derive(Clone, Debug)]
pub struct StorageEntry {
    pub data: Vec<u8>,
    pub ty: scales::TypeId,
}

/// One decoded entry in a paged storage-map response.
#[derive(Clone, Debug)]
pub struct StoragePageEntry {
    /// Complete raw storage key, suitable for use as the next exclusive cursor.
    pub raw_key: RawKey,
    /// Metadata-decoded map-key components, in declaration order.
    pub keys: Vec<StorageEntry>,
    /// Raw SCALE-encoded value, or `None` when the key disappeared before the
    /// snapshot value read completed.
    pub value: Option<StorageEntry>,
}

/// A bounded page from one finalized storage snapshot.
#[derive(Debug)]
pub struct StoragePage {
    pub at: BlockInfo,
    pub entries: Vec<StoragePageEntry>,
    /// Opaque backend cursor for the next page. `None` means the scan is
    /// complete. Feed it back unchanged; it is not necessarily a storage key.
    pub next_key: Option<RawKey>,
    /// Metadata used to decode `keys` and `value`.
    pub metadata: Rc<Metadata>,
}

impl StorageEntry {
    pub fn new(data: Vec<u8>, ty: scales::TypeId) -> Self {
        Self { data, ty }
    }

    pub fn as_value<'a>(&'a self, registry: &'a scales::Registry) -> scales::Value<'a> {
        scales::Value::new(&self.data, self.ty, registry)
    }

    /// Format the entry as a compact text string (see [`scales::to_text`]).
    pub fn to_text(&self, registry: &scales::Registry) -> Result<String> {
        scales::to_text(&self.as_value(registry)).map_err(|e| Error::Mapping(e.to_string()))
    }

    /// Decode as a little-endian u32 (block numbers, counters, etc).
    pub fn as_u32(&self) -> Option<u32> {
        self.data
            .get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Decode as a little-endian u64.
    pub fn as_u64(&self) -> Option<u64> {
        self.data
            .get(..8)
            .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
}

#[derive(Debug)]
pub enum Response {
    Void,
    None,
    Value(StorageEntry, Rc<Metadata>),
    ValueSet(Vec<(Vec<StorageEntry>, Option<StorageEntry>)>, Rc<Metadata>),
    Meta(Rc<Metadata>),
}

impl Response {
    /// Access the type registry from this response (if it carries metadata).
    pub fn registry(&self) -> Option<&scales::Registry> {
        match self {
            Response::Value(_, m) | Response::ValueSet(_, m) | Response::Meta(m) => {
                Some(&m.registry)
            }
            _ => None,
        }
    }

    /// Decode the response value as compact text.
    ///
    /// Returns `Ok(None)` for `None`/`Void`/`Meta` responses.
    pub fn to_text(&self) -> Result<Option<String>> {
        match self {
            Response::Value(entry, meta) => entry.to_text(&meta.registry).map(Some),
            _ => Ok(None),
        }
    }

    /// Extract the single storage entry, or error if not a `Value` response.
    pub fn into_value(self) -> Result<(StorageEntry, Rc<Metadata>)> {
        match self {
            Response::Value(entry, meta) => Ok((entry, meta)),
            Response::None => Err(Error::StorageKeyNotFound),
            _ => Err(Error::BadInput),
        }
    }

    /// Returns `true` if this is a `None` response (key exists but has no value).
    pub fn is_none(&self) -> bool {
        matches!(self, Response::None)
    }
}

pub type RawKey = Vec<u8>;
pub type RawValue = Vec<u8>;

/// One backend-level page of raw storage keys.
///
/// `next_cursor` is opaque: callers must feed it back to the same backend,
/// prefix, and block hash. Full-node backends commonly use the last raw key,
/// while light clients may encode bounded trie-partition progress in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawKeysPage {
    pub keys: Vec<RawKey>,
    pub next_cursor: Option<RawKey>,
}

// --- Backend trait ---

/// Generic definition of a blockchain backend.
#[allow(async_fn_in_trait)]
pub trait Backend {
    async fn get_storage_items(
        &mut self,
        keys: Vec<RawKey>,
        block: Option<u32>,
    ) -> crate::Result<Vec<(RawKey, Option<RawValue>)>>;

    async fn get_storage_item(
        &mut self,
        key: RawKey,
        block: Option<u32>,
    ) -> crate::Result<Option<RawValue>> {
        let res = self.get_storage_items(vec![key], block).await?;
        res.into_iter()
            .next()
            .map(|(_, v)| v)
            .ok_or(Error::StorageKeyNotFound)
    }

    /// Return storage values at an already-authenticated block hash.
    ///
    /// Light clients cannot securely resolve arbitrary heights to hashes, but
    /// they can verify state proofs for a finalized hash they already know.
    async fn get_storage_items_at_hash(
        &mut self,
        _keys: Vec<RawKey>,
        _block_hash: [u8; 32],
    ) -> crate::Result<Vec<(RawKey, Option<RawValue>)>> {
        Err(Error::OperationFailed(
            "hash-pinned storage is unsupported by this backend".into(),
        ))
    }

    async fn get_storage_item_at_hash(
        &mut self,
        key: RawKey,
        block_hash: [u8; 32],
    ) -> crate::Result<Option<RawValue>> {
        let res = self
            .get_storage_items_at_hash(vec![key], block_hash)
            .await?;
        res.into_iter()
            .next()
            .map(|(_, value)| value)
            .ok_or(Error::StorageKeyNotFound)
    }

    async fn get_keys_paged(
        &mut self,
        from: RawKey,
        size: u16,
        to: Option<RawKey>,
    ) -> crate::Result<Vec<RawValue>>;

    /// Return raw storage keys at a selected finalized block.
    ///
    /// Backends that cannot address historical state retain source
    /// compatibility through this default and reject an explicit block.
    async fn get_keys_paged_at(
        &mut self,
        prefix: RawKey,
        size: u16,
        start_key: Option<RawKey>,
        block: Option<u32>,
    ) -> crate::Result<Vec<RawKey>> {
        if block.is_some() {
            return Err(Error::OperationFailed(
                "block-pinned key pagination is unsupported by this backend".into(),
            ));
        }
        self.get_keys_paged(prefix, size, start_key).await
    }

    /// Return raw storage keys at an already-authenticated block hash.
    async fn get_keys_paged_at_hash(
        &mut self,
        _prefix: RawKey,
        _size: u16,
        _start_key: Option<RawKey>,
        _block_hash: [u8; 32],
    ) -> crate::Result<Vec<RawKey>> {
        Err(Error::OperationFailed(
            "hash-pinned key pagination is unsupported by this backend".into(),
        ))
    }

    /// Return a bounded page of keys at an authenticated block hash.
    ///
    /// The default adapts the legacy last-key API. Light-client backends can
    /// override this to use an opaque cursor that bounds each state proof.
    async fn get_keys_page_at_hash(
        &mut self,
        prefix: RawKey,
        limit: u16,
        cursor: Option<RawKey>,
        block_hash: [u8; 32],
    ) -> crate::Result<RawKeysPage> {
        if limit == 0 {
            return Err(Error::BadInput);
        }
        let requested = limit.checked_add(1).ok_or(Error::BadInput)?;
        let mut keys = self
            .get_keys_paged_at_hash(prefix, requested, cursor, block_hash)
            .await?;
        let has_more = keys.len() > usize::from(limit);
        keys.truncate(usize::from(limit));
        let next_cursor = has_more.then(|| keys.last().cloned()).flatten();
        Ok(RawKeysPage { keys, next_cursor })
    }

    /// Stop an in-flight backend operation or subscription after its owning
    /// future was cancelled. Backends without long-lived work have no cleanup.
    async fn cancel_active_operation(&mut self) -> Result<()> {
        Ok(())
    }

    /// Submit an extrinsic. If `wait_for_finalization` is true, waits for
    /// full finalization; otherwise returns after best-chain inclusion.
    async fn submit(&mut self, ext: &[u8], wait_for_finalization: bool) -> Result<()>;

    /// Inspect an encoded extrinsic without submitting it.
    ///
    /// Backends that expose transaction-payment and tagged-transaction-queue
    /// runtime APIs should override this. Diagnostics being unavailable is not
    /// a build failure.
    async fn inspect_transaction(&mut self, _ext: &EncodedExtrinsic) -> Result<TransactionReport> {
        Ok(TransactionReport {
            warnings: vec![
                "fee, weight, and validity runtime APIs are unavailable on this backend".into(),
            ],
            ..TransactionReport::default()
        })
    }

    /// Submit an already-built extrinsic and return its inclusion location.
    ///
    /// The default preserves compatibility with simple backends. Transaction
    /// watch backends override it to return block hash and extrinsic index.
    async fn submit_transaction(
        &mut self,
        ext: &EncodedExtrinsic,
        wait_for: WaitFor,
    ) -> Result<TransactionReceipt> {
        self.submit(&ext.bytes, matches!(wait_for, WaitFor::Finalized))
            .await?;
        Ok(TransactionReceipt::default())
    }

    /// Submit with an aggregate watch deadline. Backends that cannot enforce
    /// cancellation retain compatibility by delegating to the ordinary
    /// submission method; native chainHead backends override this.
    async fn submit_transaction_with_timeout(
        &mut self,
        ext: &EncodedExtrinsic,
        wait_for: WaitFor,
        _timeout: core::time::Duration,
    ) -> Result<TransactionReceipt> {
        self.submit_transaction(ext, wait_for).await
    }

    /// Populate metadata-decoded events and dispatch outcome for a receipt.
    async fn enrich_receipt(
        &mut self,
        receipt: TransactionReceipt,
        _metadata: &Metadata,
    ) -> Result<TransactionReceipt> {
        Ok(receipt)
    }

    /// Chain identity and denomination properties. Backends without a
    /// `system_properties` equivalent return an empty value.
    async fn chain_properties(&mut self) -> Result<ChainProperties> {
        Ok(ChainProperties::default())
    }

    async fn metadata(&mut self) -> Result<Metadata>;

    /// Fetch runtime metadata at an authenticated block hash. Backends must
    /// reject this operation when they cannot prove historical runtime state;
    /// silently returning current metadata would misdecode storage.
    async fn metadata_at_hash(&mut self, _block_hash: [u8; 32]) -> Result<Metadata> {
        Err(Error::OperationFailed(
            "hash-pinned metadata is unsupported by this backend".into(),
        ))
    }

    async fn block_info(&mut self, at: Option<u32>) -> Result<meta::BlockInfo>;

    /// Resolve and verify a header supplied by hash.
    async fn block_info_at_hash(&mut self, _block_hash: [u8; 32]) -> Result<meta::BlockInfo> {
        Err(Error::OperationFailed(
            "header lookup by hash is unsupported by this backend".into(),
        ))
    }
}

/// A dummy backend for offline querying of metadata.
pub struct Offline(pub Metadata);

impl Backend for Offline {
    async fn get_storage_items(
        &mut self,
        _keys: Vec<RawKey>,
        _block: Option<u32>,
    ) -> crate::Result<Vec<(RawKey, Option<RawValue>)>> {
        Err(Error::ChainUnavailable)
    }

    async fn get_keys_paged(
        &mut self,
        _from: RawKey,
        _size: u16,
        _to: Option<RawKey>,
    ) -> crate::Result<Vec<RawKey>> {
        Err(Error::ChainUnavailable)
    }

    async fn submit(&mut self, _ext: &[u8], _wait_for_finalization: bool) -> Result<()> {
        Err(Error::ChainUnavailable)
    }

    async fn metadata(&mut self) -> Result<Metadata> {
        Ok(self.0.clone())
    }

    async fn block_info(&mut self, _: Option<u32>) -> Result<meta::BlockInfo> {
        Err(Error::ChainUnavailable)
    }
}

// --- Error ---

#[derive(Clone, Debug)]
pub enum Error {
    ChainUnavailable,
    BadInput,
    BadMetadata,
    Decode(String),
    Encode(String),
    Node(String),
    StorageKeyNotFound,
    PalletNotFound(String),
    CallNotFound,
    MissingConstantName,
    Mapping(String),
    AccountNotFound,
    ConstantNotFound(String),
    BadBlockNumber,
    /// A hash presented as finalized is either ahead of the finalized head or
    /// is not the canonical hash at its claimed height.
    InvalidFinalizedBlock(String),
    MissingExtensionValue(String),
    /// [`TransactionOptions::with_extension`] attempted to replace an
    /// extension whose bytes are derived from the signed [`ChainContext`].
    ManagedExtensionOverride(String),
    Signing(String),
    SubscriptionClosed,
    OperationFailed(String),
    /// The transaction watch reached the terminal `invalid` state. The node
    /// rejected the extrinsic and it cannot consume the account nonce.
    TransactionInvalid(String),
    ConnectionTimeout,
    RuntimeUpgrade {
        built_spec: u32,
        current_spec: u32,
    },
    GenesisMismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChainUnavailable => write!(f, "chain unavailable"),
            Self::BadInput => write!(f, "bad input"),
            Self::BadMetadata => write!(f, "bad or missing metadata"),
            Self::Decode(e) => write!(f, "decode error: {e}"),
            Self::Encode(e) => write!(f, "encode error: {e}"),
            Self::Node(e) => write!(f, "node error: {e}"),
            Self::StorageKeyNotFound => write!(f, "storage key not found"),
            Self::PalletNotFound(p) => write!(f, "pallet not found: {p}"),
            Self::CallNotFound => write!(f, "call type not found in pallet"),
            Self::MissingConstantName => write!(f, "missing constant name in query"),
            Self::Mapping(e) => write!(f, "mapping error: {e}"),
            Self::AccountNotFound => write!(f, "account not found"),
            Self::ConstantNotFound(c) => write!(f, "constant not found: {c}"),
            Self::BadBlockNumber => write!(f, "bad block number"),
            Self::InvalidFinalizedBlock(reason) => {
                write!(f, "invalid finalized block: {reason}")
            }
            Self::MissingExtensionValue(ext) => write!(f, "missing value for extension: {ext}"),
            Self::ManagedExtensionOverride(ext) => write!(
                f,
                "extension {ext} is managed by TransactionOptions and cannot be overridden"
            ),
            Self::Signing(e) => write!(f, "signing error: {e}"),
            Self::SubscriptionClosed => write!(f, "subscription closed"),
            Self::OperationFailed(e) => write!(f, "operation failed: {e}"),
            Self::TransactionInvalid(e) => write!(f, "transaction invalid: {e}"),
            Self::ConnectionTimeout => write!(f, "connection timed out"),
            Self::RuntimeUpgrade {
                built_spec,
                current_spec,
            } => write!(
                f,
                "runtime upgraded from spec {built_spec} to {current_spec}; rebuild the transaction"
            ),
            Self::GenesisMismatch => {
                write!(f, "transaction was built for a different chain genesis")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

#[cfg(not(feature = "std"))]
impl core::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBackend {
        storage: Vec<(RawKey, Option<RawValue>)>,
        keys: Vec<RawKey>,
        last_block: Option<u32>,
        storage_calls: usize,
    }

    impl Backend for MockBackend {
        async fn get_storage_items(
            &mut self,
            keys: Vec<RawKey>,
            block: Option<u32>,
        ) -> Result<Vec<(RawKey, Option<RawValue>)>> {
            self.last_block = block;
            self.storage_calls += 1;
            Ok(keys
                .into_iter()
                .map(|key| {
                    let value = self
                        .storage
                        .iter()
                        .find(|(stored_key, _)| stored_key == &key)
                        .and_then(|(_, value)| value.clone());
                    (key, value)
                })
                .collect())
        }

        async fn get_storage_items_at_hash(
            &mut self,
            keys: Vec<RawKey>,
            _block_hash: [u8; 32],
        ) -> Result<Vec<(RawKey, Option<RawValue>)>> {
            self.get_storage_items(keys, None).await
        }

        async fn get_keys_paged(
            &mut self,
            _from: RawKey,
            size: u16,
            start_key: Option<RawKey>,
        ) -> Result<Vec<RawKey>> {
            let mut keys = self.keys.clone();
            keys.sort();
            Ok(keys
                .into_iter()
                .filter(|key| {
                    start_key
                        .as_ref()
                        .is_none_or(|start| key.as_slice() > start.as_slice())
                })
                .take(usize::from(size))
                .collect())
        }

        async fn get_keys_paged_at(
            &mut self,
            prefix: RawKey,
            size: u16,
            start_key: Option<RawKey>,
            block: Option<u32>,
        ) -> Result<Vec<RawKey>> {
            self.last_block = block;
            self.get_keys_paged(prefix, size, start_key).await
        }

        async fn get_keys_paged_at_hash(
            &mut self,
            prefix: RawKey,
            size: u16,
            start_key: Option<RawKey>,
            _block_hash: [u8; 32],
        ) -> Result<Vec<RawKey>> {
            self.get_keys_paged(prefix, size, start_key).await
        }

        async fn submit(&mut self, _ext: &[u8], _wait_for_finalization: bool) -> Result<()> {
            Err(Error::ChainUnavailable)
        }

        async fn metadata(&mut self) -> Result<Metadata> {
            Err(Error::ChainUnavailable)
        }

        async fn block_info(&mut self, at: Option<u32>) -> Result<meta::BlockInfo> {
            let number = u64::from(at.unwrap_or(77));
            Ok(meta::BlockInfo {
                number,
                hash: [number as u8; 32],
                parent: [number.saturating_sub(1) as u8; 32],
            })
        }
    }

    fn fixture_metadata() -> Rc<Metadata> {
        Rc::new(Metadata::from_bytes(include_bytes!("../tests/fixtures/kreivo.scale")).unwrap())
    }

    #[test]
    fn parse_uri_pallet_and_item() {
        let (pallet, item, keys) = parse_uri("system/account").unwrap();
        assert_eq!(pallet, "System");
        assert_eq!(item, "Account");
        assert!(keys.is_empty());
    }

    #[test]
    fn parse_uri_with_map_keys() {
        let (pallet, item, keys) = parse_uri("system/account/0x1234").unwrap();
        assert_eq!(pallet, "System");
        assert_eq!(item, "Account");
        assert_eq!(keys, vec!["0x1234"]);
    }

    #[test]
    fn parse_uri_strips_slashes() {
        let (pallet, item, _) = parse_uri("/system/account/").unwrap();
        assert_eq!(pallet, "System");
        assert_eq!(item, "Account");
    }

    #[test]
    fn parse_uri_kebab_to_camel() {
        let (pallet, item, _) = parse_uri("para-scheduler/validator-groups").unwrap();
        assert_eq!(pallet, "ParaScheduler");
        assert_eq!(item, "ValidatorGroups");
    }

    #[test]
    fn parse_uri_too_short() {
        assert!(parse_uri("system").is_none());
        assert!(parse_uri("").is_none());
    }

    #[test]
    fn parse_uri_constants() {
        let (pallet, item, keys) = parse_uri("system/_constants/Version").unwrap();
        assert_eq!(pallet, "System");
        assert_eq!(item, "_constants");
        assert_eq!(keys, vec!["Version"]);
    }

    #[test]
    fn encode_call_text_with_enum_arg() {
        use crate::extrinsic::{EncodeCall, Text};

        let meta = Metadata::from_bytes(include_bytes!("../tests/fixtures/kreivo.scale")).unwrap();
        let balances = meta.pallet_by_name("Balances").unwrap();
        let calls_ty = balances.calls_ty.unwrap();

        // Text body with MultiAddress::Id enum variant and a numeric value
        let addr = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
        let text = alloc::format!("(dest:MultiAddress::Id({addr});value:1000000000000)");
        let text_body = Text(&text);
        let encoded = text_body
            .encode_call("transfer_keep_alive", &meta.registry, calls_ty)
            .expect("text with enum arg encodes");

        assert!(
            encoded.len() > 34,
            "should have variant idx + address + value"
        );
        assert_eq!(
            encoded[2], 0xd4,
            "address starts with 0xd4 after variant + MultiAddress idx"
        );
    }

    #[test]
    fn query_uses_requested_block_and_decodes_value() {
        smol::block_on(async {
            let metadata = fixture_metadata();
            let storage_key = match resolve_query(&metadata, "system/number").unwrap() {
                ResolvedQuery::Storage(key) => key.key(),
                ResolvedQuery::Constant(_) => panic!("expected storage"),
            };
            let mut backend = MockBackend {
                storage: vec![(storage_key, Some(42u32.to_le_bytes().to_vec()))],
                keys: Vec::new(),
                last_block: None,
                storage_calls: 0,
            };

            let response = query(&mut backend, &metadata, "system/number", Some(17))
                .await
                .unwrap();
            let (entry, _) = response.into_value().unwrap();
            assert_eq!(entry.as_u32(), Some(42));
            assert_eq!(backend.last_block, Some(17));
        });
    }

    #[test]
    fn query_at_hash_decodes_without_a_height_lookup() {
        smol::block_on(async {
            let metadata = fixture_metadata();
            let storage_key = match resolve_query(&metadata, "system/number").unwrap() {
                ResolvedQuery::Storage(key) => key.key(),
                ResolvedQuery::Constant(_) => panic!("expected storage"),
            };
            let mut backend = MockBackend {
                storage: vec![(storage_key, Some(42u32.to_le_bytes().to_vec()))],
                keys: Vec::new(),
                last_block: None,
                storage_calls: 0,
            };

            let response = query_at_hash(&mut backend, &metadata, "system/number", [9; 32])
                .await
                .unwrap();
            let (entry, _) = response.into_value().unwrap();
            assert_eq!(entry.as_u32(), Some(42));
            assert_eq!(backend.last_block, None);
        });
    }

    #[test]
    fn constants_do_not_touch_the_backend() {
        smol::block_on(async {
            let metadata = fixture_metadata();
            let mut backend = MockBackend {
                storage: Vec::new(),
                keys: Vec::new(),
                last_block: None,
                storage_calls: 0,
            };

            let response = query(&mut backend, &metadata, "system/_constants/version", None)
                .await
                .unwrap();
            assert!(matches!(response, Response::Value(_, _)));
            assert_eq!(backend.storage_calls, 0);
        });
    }

    #[test]
    fn paged_map_rejects_non_transparent_key_hashers() {
        let metadata = fixture_metadata();
        let storage_key = StorageKey::new(
            0,
            Vec::new(),
            Vec::new(),
            vec![KeyValue::Empty(0)],
            vec![meta::Hasher::Blake2_256],
        );
        let error = partial_storage_entries(Vec::new(), &storage_key, &metadata).unwrap_err();
        assert!(
            matches!(error, Error::OperationFailed(message) if message.contains("non-transparent"))
        );
    }

    #[test]
    fn partial_map_query_decodes_returned_keys() {
        smol::block_on(async {
            let metadata = fixture_metadata();
            let address = "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b";
            let full_key =
                match resolve_query(&metadata, &format!("system/account/{address}")).unwrap() {
                    ResolvedQuery::Storage(key) => key.key(),
                    ResolvedQuery::Constant(_) => panic!("expected storage"),
                };
            let mut backend = MockBackend {
                storage: vec![(full_key.clone(), Some(vec![0]))],
                keys: vec![full_key],
                last_block: None,
                storage_calls: 0,
            };

            let response = query(&mut backend, &metadata, "system/account", None)
                .await
                .unwrap();
            match response {
                Response::ValueSet(entries, _) => {
                    assert_eq!(entries.len(), 1);
                    assert_eq!(entries[0].0.len(), 1);
                    assert_eq!(entries[0].0[0].data.len(), 32);
                }
                other => panic!("expected value set, got {other:?}"),
            }
        });
    }

    #[test]
    fn paged_map_query_keeps_an_exclusive_cursor_on_one_snapshot() {
        smol::block_on(async {
            let metadata = fixture_metadata();
            let mut keys = (1u8..=3)
                .map(|byte| {
                    let address = format!("0x{}", hex::encode([byte; 32]));
                    match resolve_query(&metadata, &format!("system/account/{address}")).unwrap() {
                        ResolvedQuery::Storage(key) => key.key(),
                        ResolvedQuery::Constant(_) => panic!("expected storage"),
                    }
                })
                .collect::<Vec<_>>();
            keys.sort();
            let storage = keys
                .iter()
                .cloned()
                .map(|key| (key, Some(vec![0])))
                .collect();
            let mut backend = MockBackend {
                storage,
                keys: keys.clone(),
                last_block: None,
                storage_calls: 0,
            };

            let first = query_page(&mut backend, &metadata, "system/account", 2, None, None)
                .await
                .unwrap();
            assert_eq!(first.at.number, 77);
            assert_eq!(first.entries.len(), 2);
            assert_eq!(first.entries[0].keys[0].data.len(), 32);
            assert_eq!(first.next_key, Some(keys[1].clone()));
            assert_eq!(backend.last_block, Some(77));

            let second = query_page(
                &mut backend,
                &metadata,
                "system/account",
                2,
                first.next_key,
                Some(first.at.number as u32),
            )
            .await
            .unwrap();
            assert_eq!(second.entries.len(), 1);
            assert_eq!(second.entries[0].raw_key, keys[2]);
            assert!(second.next_key.is_none());
            assert_eq!(second.at.number, 77);
        });
    }

    #[test]
    fn hash_paged_query_preserves_the_supplied_snapshot() {
        smol::block_on(async {
            let metadata = fixture_metadata();
            let mut keys = (1u8..=2)
                .map(|byte| {
                    let address = format!("0x{}", hex::encode([byte; 32]));
                    match resolve_query(&metadata, &format!("system/account/{address}")).unwrap() {
                        ResolvedQuery::Storage(key) => key.key(),
                        ResolvedQuery::Constant(_) => panic!("expected storage"),
                    }
                })
                .collect::<Vec<_>>();
            keys.sort();
            let mut backend = MockBackend {
                storage: keys
                    .iter()
                    .cloned()
                    .map(|key| (key, Some(vec![0])))
                    .collect(),
                keys: keys.clone(),
                last_block: None,
                storage_calls: 0,
            };
            let at = BlockInfo {
                number: 91,
                hash: [7; 32],
                parent: [6; 32],
            };

            let page = query_page_at_hash(
                &mut backend,
                &metadata,
                "system/account",
                1,
                None,
                at.clone(),
            )
            .await
            .unwrap();
            assert_eq!(page.at, at);
            assert_eq!(page.entries.len(), 1);
            assert_eq!(page.next_key, Some(keys[0].clone()));
            assert_eq!(backend.last_block, None);
        });
    }

    #[test]
    fn paged_query_rejects_zero_limit_and_fully_keyed_storage() {
        smol::block_on(async {
            let metadata = fixture_metadata();
            let mut backend = MockBackend {
                storage: Vec::new(),
                keys: Vec::new(),
                last_block: None,
                storage_calls: 0,
            };
            assert!(matches!(
                query_page(&mut backend, &metadata, "system/account", 0, None, None).await,
                Err(Error::BadInput)
            ));
            let address = format!("0x{}", hex::encode([1u8; 32]));
            assert!(matches!(
                query_page(
                    &mut backend,
                    &metadata,
                    &format!("system/account/{address}"),
                    1,
                    None,
                    None,
                )
                .await,
                Err(Error::BadInput)
            ));
        });
    }

    #[test]
    fn missing_storage_item_has_a_specific_error() {
        let metadata = fixture_metadata();
        let error = match resolve_query(&metadata, "system/not-a-storage-item") {
            Ok(_) => panic!("unexpected query resolution"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::StorageKeyNotFound));
    }
}
