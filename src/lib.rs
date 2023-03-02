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

pub use codec;
use codec::Encode;
pub use frame_metadata::RuntimeMetadataPrefixed;
pub use meta::Metadata;
pub use meta_ext as meta;
#[cfg(feature = "json")]
pub use scales::JsonValue;
#[cfg(feature = "v14")]
pub use scales::Value;

use async_trait::async_trait;
use core::{fmt, any::Any};
use meta::{Entry, Meta, Pallet, PalletMeta, Storage};
use prelude::*;
#[cfg(feature = "v14")]
use scale_info::PortableRegistry;
use serde::Serialize;

use crate::util::to_camel;

mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

pub type Result<T> = core::result::Result<T, Error>;
/// Surf based backend
#[cfg(any(feature = "http", feature = "http-web", feature = "js"))]
pub mod http;
/// Tungstenite based backend
#[cfg(feature = "ws")]
pub mod ws;

pub mod hasher;
pub mod meta_ext;
#[cfg(any(feature = "http", feature = "http-web", feature = "ws", feature = "js"))]
pub mod rpc;

/// ```
/// Multipurpose function to interact with a Substrate based chain
/// using a URL-path-like syntax.
///
/// # use sube::sube;
/// let backend = sube::ws::Backend::new();
/// sube
/// ```
pub async fn sube<'m>(
    chain: impl Backend,
    meta: &'m Metadata,
    path: &str,
    maybe_tx_data: Option<()>,
    signer: impl Fn(&[u8], &mut [u8]),
) -> Result<Response<'m>> {
    Ok(match path {
        "_meta" => Response::Meta(meta),
        "_meta/registry" => Response::Registry(&meta.types),
        // "_chain/block"
        _ => {
            println!("hello worl here");
            let (pallet, item_or_call, mut keys) = parse_uri(path).ok_or(Error::BadInput)?;
            println!("{:?} {:?}", pallet, item_or_call);
            let pallet = meta
                .pallet_by_name(&pallet)
                .ok_or_else(|| Error::PalletNotFound)?;
            
            if item_or_call == "_constants" {
                let const_name = keys.pop().ok_or_else(|| Error::MissingConstantName)?;
                let const_meta = pallet
                    .constants
                    .iter()
                    .find(|c| {
                        println!("name {:?} - {:?}", c.name, const_name);
                        c.name == const_name
                    })
                    .ok_or_else(|| Error::ConstantNotFound(const_name))?;

                return Ok(Response::Value(Value::new(
                    const_meta.value.clone(),
                    const_meta.ty.id(),
                    &meta.types,
                )));
            }

            if let Some(tx_data) = maybe_tx_data {
                Response::Value(Value::new(vec![], 0, &meta.types))
            } else if let Ok(key_res) = StorageKey::new(pallet, &item_or_call, &keys) {
                println!("GOT HERE MY FRIEND");
                let res = chain.query_storage(&key_res).await?;
                Response::Value(Value::new(res, key_res.ty, &meta.types))
            } else {
                Response::Value(Value::new(vec![], 0, &meta.types))
            }
        }
    })
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum Response<'m> {
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
    PalletNotFound,
    CallNotFound,
    MissingConstantName,
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
        .map(|s| s.entries().find(|e| {
            
                println!("Storage.key {:?} -  {:?}", e.name, item);
                e.name() == item
            }))
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