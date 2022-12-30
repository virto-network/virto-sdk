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

#[cfg(not(any(feature = "v12", feature = "v13", feature = "v14")))]
compile_error!("Enable one of the metadata versions");
#[cfg(all(feature = "v12", feature = "v13", feature = "v14"))]
compile_error!("Only one metadata version can be enabled at the moment");
#[cfg(all(feature = "v12", feature = "v13"))]
compile_error!("Only one metadata version can be enabled at the moment");
#[cfg(all(feature = "v12", feature = "v14"))]
compile_error!("Only one metadata version can be enabled at the moment");
#[cfg(all(feature = "v13", feature = "v14"))]
compile_error!("Only one metadata version can be enabled at the moment");

#[macro_use]
extern crate alloc;

pub use codec;
pub use frame_metadata::RuntimeMetadataPrefixed;
pub use meta::Metadata;
pub use meta_ext as meta;
#[cfg(feature = "json")]
pub use scales::JsonValue;
#[cfg(feature = "v14")]
pub use scales::Value;

use async_trait::async_trait;
use core::fmt;
use meta::{Entry, Meta, Pallet, PalletMeta, Storage};
#[cfg(feature = "std")]
use once_cell::sync::OnceCell;
#[cfg(not(feature = "std"))]
use once_cell::unsync::OnceCell;
use prelude::*;
#[cfg(feature = "v14")]
use scale_info::PortableRegistry;

mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

pub type Result<T> = core::result::Result<T, Error>;
/// Surf based backend
#[cfg(any(feature = "http", feature = "http-web"))]
pub mod http;
/// Tungstenite based backend
#[cfg(feature = "ws")]
pub mod ws;

mod hasher;
pub mod meta_ext;
#[cfg(any(feature = "http", feature = "http-web", feature = "ws"))]
mod rpc;

enum Response {
    Query,
    Tx,
    Meta,
    Registry,
}

/// ```
/// # use sube::sube;
/// let kusama = sube::ws::Backend::new();
/// sube
/// ```
pub async fn sube<'m>(
    chain: impl Backend,
    meta: &'m Metadata,
    cmd: &str,
    maybe_tx_data: Option<()>,
    signer: impl Fn(&[u8], &mut [u8]),
) -> Result<scales::Value<'m>> {
    match cmd {
        "_meta" => {}
        "_meta/registry" => {}
        // "_chain/block"
        _ => {
            let (pallet, item_or_call, keys) = parse_uri(cmd).ok_or(Error::BadInput)?;
            let pallet = meta
                .pallet_by_name(&pallet)
                .ok_or_else(|| Error::PalletNotFound)?;
            let key_res = StorageKey::new(pallet, &item_or_call, &keys);
            if let Some(tx_data) = maybe_tx_data {}
        }
    }
    Ok(Value::new(vec![], 0, &meta.types))
}

fn parse_uri(uri: &str) -> Option<(String, String, Vec<&str>)> {
    let mut path = uri.trim_matches('/').split('/');
    let pallet = path.next().map(to_camel)?;
    let item = path.next().map(to_camel)?;
    let map_keys = path.collect::<Vec<_>>();
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
    pub fn new(meta: &PalletMeta, item: &str, map_keys: &[&str]) -> Result<Self> {
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

fn to_camel(term: &str) -> String {
    let underscore_count = term.chars().filter(|c| *c == '-').count();
    let mut result = String::with_capacity(term.len() - underscore_count);
    let mut at_new_word = true;

    for c in term.chars().skip_while(|&c| c == '-') {
        if c == '-' {
            at_new_word = true;
        } else if at_new_word {
            result.push(c.to_ascii_uppercase());
            at_new_word = false;
        } else {
            result.push(c);
        }
    }
    result
}
