#![feature(async_closure)]
#![feature(trait_alias)]
#![feature(async_fn_traits)]
#![feature(impl_trait_in_assoc_type)]
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
pub use scales::{Serializer, Value, to_bytes_with_info};

use async_trait::async_trait;
use codec::Compact;
use core::fmt;
use core::ops::AsyncFn;

use hasher::hash;
use log;
use meta::{Entry, Meta, Pallet, PalletMeta, Storage};
use prelude::*;
#[cfg(feature = "v14")]
use scale_info::PortableRegistry;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[cfg(feature = "builder")]
pub use paste::paste;

use crate::{meta::Registry, util::to_camel};

mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtrinsicBody<Body> {
    pub nonce: Option<u64>,
    pub body: Body,
}

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

pub type Result<T> = core::result::Result<T, Error>;

pub trait SignerFn: AsyncFn(&[u8]) -> Result<[u8; 64]> {}
impl<T> SignerFn for T where T: AsyncFn(&[u8]) -> Result<[u8; 64]> {}

macro_rules! print_hex {
    ($e:expr) => {
        log::info!("$e hex {:?}", hex::encode(compact(&$e).encode()));
    };
}
async fn query<'m>(chain: &impl Backend, meta: &'m Metadata, path: &str) -> Result<Response<'m>> {
    log::info!("url: {:?}", path);
    let (pallet, item_or_call, mut keys) = parse_uri(path).ok_or(Error::BadInput)?;
    log::info!("item_or_call: {:?}", item_or_call);
    log::info!("keys: {:?}", keys);

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
    // let hello = to_bytes_with_info(&mut out, &key, Some((&meta.types, 38)));
    
    // log::info!("hello_encoded {:?}", hex::encode(out));

    log::info!("key {:?}", &keys);
    if let Ok(key_res) = StorageKey::new(&meta.types, pallet, &item_or_call, &keys) {
        log::info!("key_res {:?}", &key_res);
        let res = chain.query_storage(&key_res).await?;
        Ok(Response::Value(Value::new(res, key_res.ty, &meta.types)))
    } else {
        log::info!("hello here the chain is not available {:?}", &keys);
        Err(Error::ChainUnavailable)
    }
}

#[derive(Deserialize, Debug)]
pub struct AccountInfo {
    nonce: u64,
    consumers: u64,
    providers: u64,
    sufficients: u64,
    data: Data,
}

#[derive(Deserialize, Debug)]
pub struct Data {
    free: u128,
    reserved: u128,
    frozen: u128,
    flags: u128,
}

async fn submit<'m, V>(
    chain: impl Backend,
    meta: &'m Metadata,
    path: &str,
    from: &'m [u8],
    tx_data: ExtrinsicBody<V>,
    signer: impl SignerFn,
    // add chain extensions
) -> Result<Response<'m>>
where
    V: serde::Serialize + core::fmt::Debug,
{
    let (pallet, item_or_call, keys) = parse_uri(path).ok_or(Error::BadInput)?;

    let pallet = meta
        .pallet_by_name(&pallet)
        .ok_or_else(|| Error::PalletNotFound(pallet))?;

    let reg = &meta.types;

    let ty = pallet.calls().expect("pallet does not have calls").ty.id;

    let mut encoded_call = vec![pallet.index];

    let call_data = scales::to_vec_with_info(
        &json!( {
            &item_or_call.to_lowercase(): &tx_data.body
        }),
        (reg, ty).into(),
    )
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
                    &format!("system/account/0x{}", hex::encode(from)),
                )
                .await?;

                match response {
                    Response::Value(value) => {
                        log::info!("{:?}", serde_json::to_string(&value));
                        let str = serde_json::to_string(&value).expect("wrong account info");
                        let value: AccountInfo =
                            serde_json::from_str(&str).expect("it must serialize");
                        // let account_info: AccountInfo = serde_json::).unwrap();
                        log::info!("{}", &value.nonce);
                        Ok(value.nonce)
                    }
                    _ => Err(Error::AccountNotFound),
                }
            }
        }?;

        let tip: u128 = 0;

        log::info!("era {:?}", &era);
        log::info!("nonce {:?}", &nonce);
        log::info!("tip {:?}", &tip);

        log::info!("tip_hex: {}", hex::encode(Compact(tip.clone()).encode()));

        let extra_params_hex = hex::encode(
            [
                Compact(era).encode(),
                Compact(nonce).encode(),
                Compact(tip).encode(),
                vec![0x00u8],
            ]
            .concat(),
        );

        log::info!("extra_params_hex: {}", extra_params_hex);
        let extensions = ();

        // vec![0x00u8] sign extensions
        // TODO: implement signed extensions
        // meta.extrinsic.signed_extensions.iter().for_each(|ext| {
        //     log::info!("signed extension: {:?}", ext);
        //     let type_resolved = meta.types.resolve(ext.ty.id);
        //     log::info!("type_resolved: {:?}", type_resolved);
        //     log::info!("========================");
        // });

        [
            vec![era],
            Compact(nonce).encode(),
            Compact(tip).encode(),
            vec![0x00u8],
        ]
        .concat()
    };

    log::info!("Hex {:?}", hex::encode(&extra_params));

    let additional_params = {
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

        let chain_value: JsonValue = Value::new(data.value, data.ty.id, &meta.types).into();

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

        let genesis_info = chain.block_info(Some(0u32)).await?;
        log::info!("genesis_info {:?}", &genesis_info.number);
        let genesis_block: Vec<u8> = chain.block_info(Some(0u32)).await?.into();

        log::info!("spec {:?}", &spec_version);
        log::info!("transaction_version {:?}", &transaction_version);
        log::info!("genesis_block {:?}", hex::encode(&genesis_block));
        log::info!("genesis_block_hash {:?}", hex::encode(&genesis_info.hash));

        // let response = query(&chain, meta, "system/BlockHash/0").await?;
        // TODO: check why this params were not converted to 0x
        // match response {
        //     Response::Value(value) => {
        //         let str = serde_json::to_string(&value).expect("helo");
        //         log::info!("storage blockhash {:?}", str);
        //     }
        //     _ => return Err(Error::BadInput),
        // };

        [
            spec_version.to_le_bytes().to_vec(),
            transaction_version.to_le_bytes().to_vec(),
            genesis_block.clone(),
            genesis_block.clone(),
        ]
        .concat()
    };

    log::info!("encoded call {:?}", hex::encode(&encoded_call));

    log::info!("additional_params {:?}", hex::encode(&additional_params));

    let signature_payload = [
        encoded_call.clone(),
        extra_params.clone(),
        additional_params.clone(),
    ]
    .concat();

    log::info!("signature_payload {:?}", hex::encode(&signature_payload));
    let payload = if signature_payload.len() > 256 {
        hash(&meta::Hasher::Blake2_256, &signature_payload[..])
    } else {
        signature_payload
    };

    log::info!("payload.len {}", payload.len());
    log::info!("payload {:?}", hex::encode(&payload));

    let signature = signer(payload.as_slice()).await?;

    log::info!("signature {:?}", hex::encode(&signature));
    let extrinsic_call = {
        let encoded_inner = [
            // header: "is signed" (1 bites) + transaction protocol version (7 bites)
            vec![0b10000000 + 4u8], // x
            // signer
            vec![0x00], // x
            from.to_vec(),
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

    log::info!("EXTRICIC_CALL {:?}", hex::encode(&extrinsic_call));

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
    Platform(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(e) => write!(f, "{:}", e),
            _ => write!(f, "{:?}", self),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

#[cfg(feature = "no_std")]
impl core::error::Error for Error {}

/// Represents a key of the blockchain storage in its raw form
#[derive(Clone, Debug)]
pub struct StorageKey {
    key: Vec<u8>,
    pub ty: u32,
}

impl StorageKey {
    pub fn new<T: AsRef<str>>(registry: &PortableRegistry, meta: &PalletMeta, item: &str, map_keys: &[T]) -> Result<Self> {
        let entry = meta
            .storage()
            .map(|s| s.entries().find(|e| {
                log::info!("e.name {:?}", e.name());
                e.name() == item
            }))
            .flatten()
            .ok_or(Error::StorageKeyNotFound)?;


        let key = entry
            .key(&registry, meta.name(), map_keys)
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
