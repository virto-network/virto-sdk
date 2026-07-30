#![cfg_attr(not(feature = "std"), no_std)]
/*!
Lightweight Substrate client focused on size and portability.
Runs in `no_std` (including embedded Cortex-M), browser, and standard environments.

Uses runtime metadata (≥ v15) and [`scales`] to convert between SCALE binary
and human-readable formats (JSON, text) without hardcoded type information.

# Usage

```no_run
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

pub use builder::{CallBuilder, OneShotCall, Sube, SubeBuilder};
#[cfg(feature = "ws-edge")]
pub use builder::{EdgeResources, EdgeSube, connect_edge};
pub use extrinsic::{EncodeCall, Text};
pub use meta::Metadata;
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
pub use rpc::chainhead::{BlockHeader, ChainEvent, ChainSession};
pub use signer::{Bytes, ExtrinsicAssembler, Signer, SignerFn};

use core::fmt;
use metadata::{self as meta, KeyValue, StorageKey};
use prelude::*;
use util::to_camel;

mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

#[cfg(any(feature = "ws", feature = "smoldot-std"))]
pub mod backend;
pub mod builder;
pub mod extrinsic;
mod hasher;
pub mod metadata;
pub mod rpc;
pub mod signer;
pub mod util;
pub mod value;

/// Connect to a Substrate chain.
///
/// Returns a [`SubeBuilder`] — `.await` it to get a connected [`Sube`] handle.
///
/// ```no_run
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

pub(crate) async fn query(
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

fn partial_storage_response(
    values: Vec<(RawKey, Option<RawValue>)>,
    storage_key: &StorageKey,
    meta: &Rc<Metadata>,
) -> Result<Response> {
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

        decoded.push((
            parts,
            value.map(|data| StorageEntry::new(data, storage_key.ty)),
        ));
    }

    Ok(Response::ValueSet(decoded, Rc::clone(meta)))
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

    async fn get_keys_paged(
        &mut self,
        from: RawKey,
        size: u16,
        to: Option<RawKey>,
    ) -> crate::Result<Vec<RawValue>>;

    /// Submit an extrinsic. If `wait_for_finalization` is true, waits for
    /// full finalization; otherwise returns after best-chain inclusion.
    async fn submit(&mut self, ext: &[u8], wait_for_finalization: bool) -> Result<()>;

    async fn metadata(&mut self) -> Result<Metadata>;

    async fn block_info(&mut self, at: Option<u32>) -> Result<meta::BlockInfo>;
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
    MissingExtensionValue(String),
    Signing(String),
    SubscriptionClosed,
    OperationFailed(String),
    ConnectionTimeout,
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
            Self::MissingExtensionValue(ext) => write!(f, "missing value for extension: {ext}"),
            Self::Signing(e) => write!(f, "signing error: {e}"),
            Self::SubscriptionClosed => write!(f, "subscription closed"),
            Self::OperationFailed(e) => write!(f, "operation failed: {e}"),
            Self::ConnectionTimeout => write!(f, "connection timed out"),
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

        async fn get_keys_paged(
            &mut self,
            _from: RawKey,
            _size: u16,
            _to: Option<RawKey>,
        ) -> Result<Vec<RawKey>> {
            Ok(self.keys.clone())
        }

        async fn submit(&mut self, _ext: &[u8], _wait_for_finalization: bool) -> Result<()> {
            Err(Error::ChainUnavailable)
        }

        async fn metadata(&mut self) -> Result<Metadata> {
            Err(Error::ChainUnavailable)
        }

        async fn block_info(&mut self, _at: Option<u32>) -> Result<meta::BlockInfo> {
            Err(Error::ChainUnavailable)
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
    fn missing_storage_item_has_a_specific_error() {
        let metadata = fixture_metadata();
        let error = match resolve_query(&metadata, "system/not-a-storage-item") {
            Ok(_) => panic!("unexpected query resolution"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::StorageKeyNotFound));
    }
}
