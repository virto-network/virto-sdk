#![feature(trait_alias)]
#![feature(type_alias_impl_trait)]
#![cfg_attr(not(feature = "std"), no_std)]
/*!
Sube is a lightweight Substrate client with multi-backend support
that can use a chain's type information to auto encode/decode data
into human-readable formats like JSON.

## Usage

Sube requires one of the metadata versions to be enabled(default: `v14`).
You can change it activating the relevant feature.
You will also likely want to activate a backend(default: `ws`).

```toml
[dependencies]
sube = { version = "0.4", default_features = false, features = ["v13", "http"] }
```

Creating a client is as simple as instantiating a backend and converting it to a `Sube` instance.

```
# use sube::{Sube, ws, JsonValue, Error, meta::*, Backend};
# #[async_std::main] async fn main() -> Result<(), Error> {
# const CHAIN_URL: &str = "ws://localhost:24680";
// Create an instance of Sube from any of the available backends
let client: Sube<_> = ws::Backend::new_ws2(CHAIN_URL).await?.into();

// With the client you can:
// - Inspect its metadata
let meta = client.metadata().await?;
let system = meta.pallet_by_name("System").unwrap();
assert_eq!(system.index, 0);

// - Query the chain storage with a path-like syntax
let latest_block: JsonValue = client.query("system/number").await?.into();
assert!(
    latest_block.as_u64().unwrap() > 0,
    "block {} is greater than 0",
    latest_block
);

// - Submit a signed extrinsic
# // const SIGNED_EXTRINSIC: [u8; 6] = hex_literal::hex!("ff");
// client.submit(SIGNED_EXTRINSIC).await?;
# Ok(()) }
```

### Backend features

* **http** -
  Enables a surf based http backend.
* **http-web** -
  Enables surf with its web compatible backend that uses `fetch` under the hood(target `wasm32-unknown-unknown`)
* **ws** -
  Enables the websocket backend based on tungstenite
* **wss** -
  Same as `ws` and activates the TLS functionality of tungstenite

*/

#[cfg(not(any(feature = "v13", feature = "v14")))]
compile_error!("Enable one of the metadata versions");
#[cfg(all(feature = "v13", feature = "v14"))]
compile_error!("Only one metadata version can be enabled at the moment");

#[macro_use]
extern crate alloc;

pub mod util;

use builder::SignerFn;
pub use codec;
use codec::{Decode, Encode};
pub use frame_metadata::RuntimeMetadataPrefixed;

pub use meta::Metadata;
pub use meta_ext as meta;
#[cfg(feature = "json")]
pub use scales::JsonValue;
#[cfg(feature = "v14")]
pub use scales::{Serializer, Value};

use async_trait::async_trait;
use codec::Compact;
use core::{
    any::Any,
    borrow::{Borrow, BorrowMut},
    fmt,
};
use hasher::hash;
use meta::{Entry, Meta, Pallet, PalletMeta, Storage};
use prelude::*;
#[cfg(feature = "v14")]
use scale_info::PortableRegistry;
use serde::{Deserialize, Serialize};

use crate::util::to_camel;

mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

#[derive(Serialize, Deserialize)]
pub struct ExtrinicBody<Body> {
    pub nonce: Option<u64>,
    pub body: Body,
    pub from: Vec<u8>,
}

pub type Result<T> = core::result::Result<T, Error>;

/// Surf based backend
#[cfg(any(feature = "http", feature = "http-web", feature = "js"))]
pub mod http;
/// Tungstenite based backend
#[cfg(feature = "ws")]
pub mod ws;

#[cfg(any(feature = "builder"))]
pub mod builder;
pub mod hasher;
pub mod meta_ext;

#[cfg(any(feature = "http", feature = "http-web", feature = "ws", feature = "js"))]
pub mod rpc;

#[derive(Deserialize, Decode)]
struct ChainVersion {
    spec_version: u64,
    transaction_version: u64,
}

#[derive(Deserialize, Serialize, Decode)]
struct AccountInfo {
    nonce: u64,
}

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// ```
/// Multipurpose function to interact with a Substrate based chain
/// using a URL-path-like syntax.
///
/// # use sube::sube;
/// let backend = sube::ws::Backend::new();
/// sube
///
/// ```
pub async fn exec<'m, Body, P: Into<&'m str>, S>(
    chain: impl Backend,
    meta: &'m Metadata,
    path: P,
    maybe_tx_data: Option<ExtrinicBody<Body>>,
    signer: impl SignerFn,
) -> Result<Response<'m>>
where
    Body: serde::Serialize,
{
    let path = path.into();

    Ok(match path {
        "_meta" => Response::Meta(meta),
        "_meta/registry" => Response::Registry(&meta.types),
        _ => {
            if let Some(tx_data) = maybe_tx_data {
                submit(chain, meta, path, tx_data, signer).await?
            } else {
                query(&chain, meta, path).await?
            }
        }
    })
}

async fn query<'m>(chain: &impl Backend, meta: &'m Metadata, path: &str) -> Result<Response<'m>> {
    let (pallet, item_or_call, mut keys) = parse_uri(path).ok_or(Error::BadInput)?;

    let pallet = meta
        .pallet_by_name(&pallet)
        .ok_or_else(|| Error::PalletNotFound(pallet))?;

    if item_or_call == "_constants" {
        let const_name = keys.pop().ok_or_else(|| Error::MissingConstantName)?;
        let const_meta = pallet
            .constants
            .iter()
            .find(|c| c.name == const_name)
            .ok_or_else(|| Error::ConstantNotFound(const_name))?;

        return Ok(Response::Value(Value::new(
            const_meta.value.clone(),
            const_meta.ty.id(),
            &meta.types,
        )));
    }

    if let Ok(key_res) = StorageKey::new(pallet, &item_or_call, &keys) {
        let res = chain.query_storage(&key_res).await?;
        Ok(Response::Value(Value::new(res, key_res.ty, &meta.types)))
    } else {
        Err(Error::ChainUnavailable)
    }
}

async fn submit<'m, V>(
    chain: impl Backend,
    meta: &'m Metadata,
    path: &str,
    tx_data: ExtrinicBody<V>,
    signer: impl SignerFn,
) -> Result<Response<'m>>
where
    V: serde::Serialize,
{
    let (pallet, item_or_call, keys) = parse_uri(path).ok_or(Error::BadInput)?;

    let pallet = meta
        .pallet_by_name(&pallet)
        .ok_or_else(|| Error::PalletNotFound(pallet))?;

    let reg = &meta.types;

    let ty = pallet.calls().expect("pallet does not have calls").ty.id();

    let mut encoded_call = vec![pallet.index];

    let call_data = scales::to_vec_with_info(&tx_data.body, (reg, ty).into())
        .map_err(|e| Error::Encode(e.to_string()))?;

    encoded_call.extend(&call_data);

    let extra_params = {
        // ImmortalEra
        let era = 0u8;

        // Impl. Note: in a real-world use case, you should store your account's nonce somewhere else
        let nonce = {
            if let Some(nonce) = tx_data.nonce {
                Ok(nonce)
            } else {
                let response = query(
                    &chain,
                    meta,
                    &format!("system/account/0x{}", hex::encode(&tx_data.from)),
                )
                .await?;

                match response {
                    Response::Value(value) => {
                        let bytes: [u8; 8] = value.as_ref()[..8].try_into().expect("fits");
                        let nonce = u64::from_le_bytes(bytes);
                        Ok(nonce)
                    }
                    _ => Err(Error::AccountNotFound),
                }
            }
        }?;

        let tip: u128 = 0;

        [vec![era], Compact(nonce).encode(), Compact(tip).encode()].concat()
    };

    let additional_params = {
        // Error: Still failing to deserialize the const
        let metadata = meta;

        let mut constants = metadata
            .pallet_by_name("System")
            .ok_or(Error::PalletNotFound(String::from("System")))?
            .constants
            .clone()
            .into_iter();

        let data = constants
            .find(|c| c.name == "Version")
            .ok_or(Error::ConstantNotFound("System_Version".into()))?;

        let chain_value: JsonValue = Value::new(data.value, data.ty.id(), &meta.types).into();

        let iter = chain_value
            .as_object()
            .ok_or(Error::ConstantNotFound("System_Version".into()))?;

        let transaction_version = iter.get("transaction_version").ok_or(Error::Mapping(
            "System_Version.transaction_version not found in transaction version".into(),
        ))?;

        let spec_version = iter.get("spec_version").ok_or(Error::Mapping(
            "System_Version.spec_version not found in transaction version".into(),
        ))?;
        // chain_version

        let spec_version = spec_version.as_u64().ok_or(Error::Mapping(
            "System_Version.spec_version is not a number".into(),
        ))? as u32;

        let transaction_version = transaction_version.as_u64().ok_or(Error::Mapping(
            "System_Version.transaction_version is not a number".into(),
        ))? as u32;

        let genesis_block: Vec<u8> = chain.block_info(Some(0u32)).await?.into();

        [
            spec_version.to_le_bytes().to_vec(),
            transaction_version.to_le_bytes().to_vec(),
            genesis_block.clone(),
            genesis_block.clone(),
        ]
        .concat()
    };

    let signature_payload = [
        encoded_call.clone(),
        extra_params.clone(),
        additional_params.clone(),
    ]
    .concat();

    let payload = if signature_payload.len() > 256 {
        hash(&meta::Hasher::Blake2_256, &signature_payload[..])
    } else {
        signature_payload
    };

    let raw = payload.as_slice();
    let mut signature: [u8; 64] = [0u8; 64];

    signer(raw, &mut signature)?;

    let extrinsic_call = {
        let encoded_inner = [
            // header: "is signed" (1 byte) + transaction protocol version (7 bytes)
            vec![0b10000000 + 4u8],
            // signer
            vec![0x00],
            tx_data.from.to_vec(),
            // signature
            [vec![0x01], signature.to_vec()].concat(),
            // extra
            extra_params,
            // call data
            encoded_call,
        ]
        .concat();

        let len = Compact(
            u32::try_from(encoded_inner.len()).expect("extrinsic size expected to be <4GB"),
        )
        .encode();

        [len, encoded_inner].concat()
    };

    chain.submit(&extrinsic_call).await?;

    Ok(Response::Void)
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum Response<'m> {
    Void,
    Value(scales::Value<'m>),
    Meta(&'m Metadata),
    Registry(&'m PortableRegistry),
}

impl From<Response<'_>> for Vec<u8> {
    fn from(res: Response) -> Self {
        match res {
            Response::Value(v) => v.as_ref().into(),
            Response::Meta(m) => m.encode(),
            Response::Registry(r) => r.encode(),
            Response::Void => vec![0],
        }
    }
}

fn parse_uri(uri: &str) -> Option<(String, String, Vec<String>)> {
    let mut path = uri.trim_matches('/').split('/');
    let pallet = path.next().map(to_camel)?;
    let item = path.next().map(to_camel)?;
    let map_keys = path.map(to_camel).collect::<Vec<_>>();
    Some((pallet, item, map_keys))
}

struct PalletCall {
    pallet_idx: u8,
    ty: u32,
}

impl PalletCall {
    fn new(pallet: &PalletMeta, reg: &PortableRegistry, call: &str) -> Result<Self> {
        let calls = pallet
            .calls
            .as_ref()
            .map(|c| reg.resolve(c.ty.id()))
            .flatten()
            .ok_or_else(|| Error::CallNotFound)?
            .type_def();
        log::debug!("{:?}", calls);
        let pallet_idx = pallet.index;
        Ok(PalletCall { pallet_idx, ty: 0 })
    }
}

/// Generic definition of a blockchain backend
///
/// ```rust,ignore
/// #[async_trait]
/// pub trait Backend {
///     async fn query_bytes(&self, key: &StorageKey) -> Result<Vec<u8>>;
///
///     async fn submit<T>(&self, ext: T) -> Result<()>
///     where
///         T: AsRef<[u8]> + Send;
///
///     async fn metadata(&self) -> Result<Metadata>;
/// }
/// ```
#[async_trait]
pub trait Backend {
    /// Get raw storage items form the blockchain
    async fn query_storage(&self, key: &StorageKey) -> Result<Vec<u8>>;

    /// Send a signed extrinsic to the blockchain
    async fn submit<T>(&self, ext: T) -> Result<()>
    where
        T: AsRef<[u8]> + Send;

    async fn metadata(&self) -> Result<Metadata>;

    async fn block_info(&self, at: Option<u32>) -> Result<meta::BlockInfo>;
}

/// A Dummy backend for offline querying of metadata
pub struct Offline(pub Metadata);

#[async_trait]
impl Backend for Offline {
    async fn query_storage(&self, _key: &StorageKey) -> Result<Vec<u8>> {
        Err(Error::ChainUnavailable)
    }

    /// Send a signed extrinsic to the blockchain
    async fn submit<T>(&self, _ext: T) -> Result<()>
    where
        T: AsRef<[u8]> + Send,
    {
        Err(Error::ChainUnavailable)
    }

    async fn metadata(&self) -> Result<Metadata> {
        Ok(self.0.cloned())
    }

    async fn block_info(&self, _: Option<u32>) -> Result<meta::BlockInfo> {
        Err(Error::ChainUnavailable)
    }
}

#[derive(Clone, Debug)]
pub enum Error {
    ChainUnavailable,
    BadInput,
    BadKey,
    BadMetadata,
    Decode(codec::Error),
    Encode(String),
    NoMetadataLoaded,
    Node(String),
    ParseStorageItem,
    StorageKeyNotFound,
    PalletNotFound(String),
    CallNotFound,
    MissingConstantName,
    Signing,
    Mapping(String),
    AccountNotFound,
    ConstantNotFound(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(e) => write!(f, "{:}", e),
            _ => write!(f, "{:?}", self),
        }
    }
}

#[cfg(feature = "ws")]
impl From<async_tungstenite::tungstenite::Error> for Error {
    fn from(_err: async_tungstenite::tungstenite::Error) -> Self {
        Error::ChainUnavailable
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// Represents a key of the blockchain storage in its raw form
#[derive(Clone, Debug)]
pub struct StorageKey {
    key: Vec<u8>,
    pub ty: u32,
}

impl StorageKey {
    pub fn new<T: AsRef<str>>(meta: &PalletMeta, item: &str, map_keys: &[T]) -> Result<Self> {
        let entry = meta
            .storage()
            .map(|s| s.entries().find(|e| e.name() == item))
            .flatten()
            .ok_or(Error::StorageKeyNotFound)?;

        let key = entry
            .key(meta.name(), map_keys)
            .ok_or(Error::StorageKeyNotFound)?;

        Ok(StorageKey {
            key,
            ty: entry.ty_id(),
        })
    }
}

impl fmt::Display for StorageKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.key))
    }
}

impl AsRef<[u8]> for StorageKey {
    fn as_ref(&self) -> &[u8] {
        self.key.as_ref()
    }
}
