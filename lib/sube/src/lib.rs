#![cfg_attr(not(feature = "std"), no_std)]
/*!
Sube is a lightweight blockchain client to query and submit extrinsics
to Substrate based blockchains.
It supports multiple backends and uses the chain's type information
to automatically encode/decode data into human-readable formats like JSON.

# Usage

```rust,ignore
let chain = sube("wss://kreivo.io").await?;

// Query storage
let result = chain.query("system/account/0x1234...").await?;

// Submit an extrinsic
chain.call("balances/transfer")
    .body(json!({ "dest": {"Id": dest}, "value": 1000 }))
    .signer(my_signer)
    .await?;
```

# Feature Flags

| Feature | Description |
|---------|-------------|
| `http` | HTTP backend via `reqwest` (native) |
| `http-web` | HTTP backend via `reqwest` with WASM/`wasm-bindgen` support |
| `json` | Enable JSON serialization support in `scales` |
| `text` | Enable compact text format support in `scales` |
| `std` | Enable standard library support across dependencies |
| `ws` | WebSocket backend via `ewebsock` and `smol` |
| `wss` | WebSocket backend with TLS support (implies `ws`) |
| `smoldot` | Embedded light client backend via `smoldot-light` |
| `smoldot-std` | Smoldot with standard library and `smol` (implies `smoldot` + `std`) |
| `js` | Bundle of features for browser/WASM targets (`http-web` + `json` + `wss`) |
*/

#[macro_use]
extern crate alloc;

pub use codec;
pub use core::fmt::Display;
pub use scales::{self, Registry, Serializer, Value};
pub use serde_json::{json, Value as JsonValue};

pub use builder::{CallBuilder, OneShotCall, Sube, SubeBuilder};
pub use extrinsic::{EncodeCall, ExtrinsicBody, Text};
pub use meta::Metadata;
pub use rpc::{HttpTransport, Rpc, RpcClient};
pub use signer::{Bytes, Signer, SignerFn};

use core::fmt;
use metadata::{self as meta, KeyValue, StorageKey};
use prelude::*;
use serde::{Deserialize, Serialize};
use util::to_camel;

mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

pub(crate) mod backend;
pub mod builder;
pub mod extrinsic;
mod hasher;
pub mod metadata;
pub mod rpc;
mod signer;
/// Public subscription types
#[cfg(any(feature = "ws", feature = "smoldot"))]
pub mod subscription;
pub(crate) mod url;
pub mod util;

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

pub async fn query(
    chain: &mut (impl Backend + ?Sized),
    meta: &'static Metadata,
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
            &meta.registry,
        ));
    }

    if let Ok(key_res) =
        StorageKey::build_with_registry(&meta.registry, pallet, &item_or_call, &keys)
    {
        if !key_res.is_partial() {
            let res = chain.get_storage_item(key_res.key(), block).await?;

            let value = match res {
                None => Response::None,
                Some(res) => Response::Value(StorageEntry::new(res, key_res.ty), &meta.registry),
            };

            return Ok(value);
        }

        let res = chain.get_keys_paged(key_res.key(), 1000, None).await?;
        let result = chain.get_storage_items(res, block).await?;

        let value = result
            .into_iter()
            .map(|(key, data)| {
                let key = &key[(key_res.pallet.len() + key_res.call.len())..];
                let mut offset = 16; // TODO depends on the hasher used
                let keys = key_res
                    .args
                    .iter()
                    .map(|arg| match arg {
                        KeyValue::Empty(type_id) | KeyValue::Value((type_id, _, _, _)) => {
                            let entry = StorageEntry::new(key[offset..].to_vec(), *type_id);
                            let size = entry.as_value(&meta.registry).size().unwrap_or(0);
                            offset += size + 16;
                            entry
                        }
                    })
                    .collect::<Vec<StorageEntry>>();

                let value = data.map(|data| StorageEntry::new(data, key_res.ty));
                (keys, value)
            })
            .collect::<Vec<_>>();

        Ok(Response::ValueSet(value, &meta.registry))
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

    pub fn to_json(&self, registry: &scales::Registry) -> Result<JsonValue> {
        serde_json::to_value(self.as_value(registry)).map_err(|e| Error::Mapping(e.to_string()))
    }

    /// Format the entry as a compact text string (see [`scales::to_text`]).
    pub fn to_text(&self, registry: &scales::Registry) -> Result<String> {
        scales::to_text(&self.as_value(registry)).map_err(|e| Error::Mapping(e.to_string()))
    }
}

#[derive(Debug)]
pub enum Response {
    Void,
    None,
    Value(StorageEntry, &'static scales::Registry),
    ValueSet(
        Vec<(Vec<StorageEntry>, Option<StorageEntry>)>,
        &'static scales::Registry,
    ),
    Meta(&'static Metadata),
    Registry(&'static scales::Registry),
}

impl From<Response> for Vec<u8> {
    fn from(res: Response) -> Self {
        match res {
            Response::Value(v, _) => v.data,
            Response::None => vec![0],
            Response::Meta(m) => serde_json::to_vec(m).unwrap_or_default(),
            Response::ValueSet(_, _) => vec![],
            Response::Void => vec![],
            Response::Registry(_) => vec![],
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct StorageChangeSet {
    block: String,
    changes: Vec<(String, Option<String>)>,
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

    async fn submit(&mut self, ext: &[u8]) -> Result<()>;

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

    async fn submit(&mut self, _ext: &[u8]) -> Result<()> {
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
    fn storage_entry_to_json() {
        let meta = Metadata::from_bytes(include_bytes!(
            "../../../sdk/js/.papi/metadata/kreivo.scale"
        ))
        .unwrap();
        let system = meta.pallet_by_name("System").unwrap();
        let version = system
            .constants
            .iter()
            .find(|c| c.name == "Version")
            .unwrap();
        let entry = StorageEntry::new(version.value.clone(), version.ty);
        let json = entry.to_json(&meta.registry).expect("decodes to JSON");
        assert!(json.get("spec_name").is_some());
    }

    #[test]
    fn encode_call_json_and_text_match() {
        use crate::extrinsic::{EncodeCall, Text};

        let meta = Metadata::from_bytes(include_bytes!(
            "../../../sdk/js/.papi/metadata/kreivo.scale"
        ))
        .unwrap();
        let system = meta.pallet_by_name("System").unwrap();
        let calls_ty = system.calls_ty.unwrap();

        // JSON body for system::remark
        let json_body = serde_json::json!({ "remark": "0x68656c6c6f" });
        let json_encoded = json_body
            .encode_call("remark", &meta.registry, calls_ty)
            .expect("json encodes");

        // Text body for the same call
        let text_body = Text("(remark:0x68656c6c6f)");
        let text_encoded = text_body
            .encode_call("remark", &meta.registry, calls_ty)
            .expect("text encodes");

        assert_eq!(
            json_encoded, text_encoded,
            "JSON and text format should produce identical SCALE bytes"
        );
    }

    #[test]
    fn encode_call_text_with_enum_arg() {
        use crate::extrinsic::{EncodeCall, Text};

        let meta = Metadata::from_bytes(include_bytes!(
            "../../../sdk/js/.papi/metadata/kreivo.scale"
        ))
        .unwrap();
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
