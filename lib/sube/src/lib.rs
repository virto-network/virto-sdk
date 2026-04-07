#![cfg_attr(not(feature = "std"), no_std)]
/*!
Lightweight Substrate client focused on size and portability.
Runs in `no_std` (including embedded Cortex-M), browser, and standard environments.

Uses runtime metadata (≥ v15) and [`scales`] to convert between SCALE binary
and human-readable formats (JSON, text) without hardcoded type information.

# Usage

```rust,ignore
use sube::sube;

// One-liner query
let r = sube("wss://kreivo.io/system/account/0x1234").await?;

// Reusable handle
let mut chain = sube::Sube::connect("wss://kreivo.io").await?;
let r = chain.query("system/account/0x1234").await?;

// Historical block query (via archive API)
let old = chain.query_at("system/account/0x1234", 1000).await?;

// Submit extrinsic (waits for finalization)
chain.call("balances/transfer_keep_alive")
    .body(json!({ "dest": {"Id": dest}, "value": 1000 }))
    .signer(my_signer)
    .await?;
```

# Backends

| Feature | Description |
|---------|-------------|
| `ws` | WebSocket via `async-tungstenite` + `smol` (std) |
| `wss` | WebSocket with TLS (implies `ws`) |
| `ws-edge` | WebSocket via `edge-ws` for embedded targets (no_std) |
| `smoldot-std` | Embedded light client via `smoldot-light` (no external node) |

# Other Features

| Feature | Description |
|---------|-------------|
| `json` | JSON serialization via `scales` |
| `text` | Compact text format via `scales` |
| `std` | Standard library support |
*/

#[macro_use]
extern crate alloc;

pub use alloc::rc::Rc;
pub use scales::{self, Registry, Value};
pub use value::DynValue;

#[cfg(feature = "ws-edge")]
pub use builder::{connect_edge, EdgeNet, EdgeSube};
pub use builder::{CallBuilder, OneShotCall, Sube, SubeBuilder};
pub use extrinsic::{EncodeCall, Text};
pub use meta::Metadata;
#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
pub use rpc::chainhead::{BlockHeader, ChainEvent, ChainSession};
pub use signer::{Bytes, Signer, SignerFn};

use core::fmt;
use metadata::{self as meta, KeyValue, StorageKey};
use prelude::*;
use util::to_camel;

mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

#[cfg(any(feature = "ws", feature = "smoldot"))]
pub mod backend;
pub mod builder;
pub(crate) mod extrinsic;
mod hasher;
pub mod metadata;
pub mod rpc;
mod signer;
pub(crate) mod util;
pub mod value;

/// Connect to a Substrate chain.
///
/// Returns a [`SubeBuilder`] — `.await` it to get a connected [`Sube`] handle.
///
/// ```rust,ignore
/// let chain = sube("wss://kreivo.io").await?;
/// let response = chain.query("system/account/0x1234").await?;
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
    let (pallet, item_or_call, mut keys) = parse_uri(path).ok_or(Error::BadInput)?;
    let pallet = meta
        .pallet_by_name(&pallet)
        .ok_or(Error::PalletNotFound(pallet))?;

    if item_or_call == "_constants" {
        let const_name = keys.pop().ok_or(Error::MissingConstantName)?;
        let const_meta = pallet
            .constants
            .iter()
            .find(|c| c.name == const_name)
            .ok_or(Error::ConstantNotFound(const_name))?;

        return Ok(Response::Value(
            StorageEntry::new(const_meta.value.clone(), const_meta.ty),
            Rc::clone(meta),
        ));
    }

    if let Ok(key_res) =
        StorageKey::build_with_registry(&meta.registry, pallet, &item_or_call, &keys)
    {
        if !key_res.is_partial() {
            let res = chain.get_storage_item(key_res.key(), block).await?;

            let value = match res {
                None => Response::None,
                Some(res) => Response::Value(StorageEntry::new(res, key_res.ty), Rc::clone(meta)),
            };

            return Ok(value);
        }

        let res = chain.get_keys_paged(key_res.key(), 1000, None).await?;
        let result = chain.get_storage_items(res, block).await?;

        let value = result
            .into_iter()
            .map(|(key, data)| {
                let key = &key[(key_res.pallet.len() + key_res.call.len())..];
                let mut offset = 0;
                let keys = key_res
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, arg)| {
                        let type_id = match arg {
                            KeyValue::Empty(ty) | KeyValue::Value((ty, _, _, _)) => *ty,
                        };
                        let hasher = &key_res.hashers[i];
                        let prefix_len = hasher.key_prefix_len();
                        offset += prefix_len;
                        let entry = StorageEntry::new(key[offset..].to_vec(), type_id);
                        let size = entry.as_value(&meta.registry).size().unwrap_or(0);
                        offset += size;
                        entry
                    })
                    .collect::<Vec<StorageEntry>>();

                let value = data.map(|data| StorageEntry::new(data, key_res.ty));
                (keys, value)
            })
            .collect::<Vec<_>>();

        Ok(Response::ValueSet(value, Rc::clone(meta)))
    } else {
        Err(Error::ChainUnavailable)
    }
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

impl From<Response> for Vec<u8> {
    fn from(res: Response) -> Self {
        match res {
            Response::Value(v, _) => v.data,
            Response::None => vec![0],
            Response::Meta(_) => vec![],
            Response::ValueSet(_, _) => vec![],
            Response::Void => vec![],
        }
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
}
