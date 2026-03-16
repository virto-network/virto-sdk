#![cfg_attr(not(feature = "std"), no_std)]
#![allow(async_fn_in_trait)]
/*!
Sube is a lightweight blockchain client to query and submit extrinsics
to Substrate based blockchains.
It supports multiple backends and uses the chain's type information
to automatically encode/decode data into human-readable formats like JSON.

TODO: rewrite docs for sube 1.0
*/

#[macro_use]
extern crate alloc;

pub use codec;
use codec::Encode;
pub use core::fmt::Display;
use core::iter::Empty;

pub use signer::{Bytes, Signer, SignerFn};

pub use meta::Metadata;
pub use scales::{Serializer, Value};
pub use scales::Registry;

use codec::Compact;
use core::fmt;
use hasher::hash;
// use meta::Meta;
use metadata::{self as meta};
use metadata::{KeyValue, StorageKey};
use prelude::*;
use serde::{Deserialize, Serialize};
pub use serde_json::{json, Value as JsonValue};

use crate::util::to_camel;

mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

/// Surf based backend
#[cfg(any(feature = "http", feature = "http-web"))]
pub mod http;
/// Tungstenite based backend
#[cfg(feature = "ws")]
pub mod ws;

pub mod builder;
pub use builder::SubeBuilder;
mod hasher;
pub mod metadata;
mod signer;

#[cfg(any(feature = "http", feature = "http-web", feature = "ws"))]
pub mod rpc;
pub mod util;

/// The batteries included way to query or submit extrinsics to a Substrate based blockchain
///
/// Returns a builder that implments `IntoFuture` so it can be `.await`ed on.
pub fn sube(url: &str) -> builder::SubeBuilder<'_, (), ()> {
    builder::SubeBuilder::default().with_url(url)
}

pub type Result<T> = core::result::Result<T, Error>;

async fn query<'m>(
    chain: &impl Backend,
    meta: &'m Metadata,
    path: &str,
    block: Option<u32>,
) -> Result<Response<'m>> {
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
                let mut offset = 16; // TODO it depends on the hasher used to encode the key, then the size could change
                let keys = key_res
                    .args
                    .iter()
                    .map(|arg| match arg {
                        KeyValue::Empty(type_id) | KeyValue::Value((type_id, _, _, _)) => {
                            let entry = StorageEntry::new(key[offset..].to_vec(), *type_id);
                            let size = entry
                                .as_value(&meta.registry)
                                .size()
                                .unwrap_or(0);
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

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtrinsicBody<Body> {
    pub nonce: Option<u64>,
    pub body: Body,
    #[serde(default)]
    pub extensions: Vec<(String, JsonValue)>,
}

/// Chain context fetched once for extension defaults.
struct ChainContext {
    spec_version: u32,
    tx_version: u32,
    genesis_hash: [u8; 32],
    account_nonce: u64,
}

/// Look up a caller-provided extension value by identifier.
fn find_extension_override(extensions: &[(String, JsonValue)], id: &str) -> Option<JsonValue> {
    extensions
        .iter()
        .find(|(k, _)| k == id)
        .map(|(_, v)| v.clone())
}

/// Returns a default JSON value for well-known extension "extra" data.
fn default_extra_value(identifier: &str, ctx: &ChainContext) -> Option<JsonValue> {
    match identifier {
        "CheckMortality" => Some(json!({"Immortal": null})),
        "CheckNonce" => Some(json!(ctx.account_nonce)),
        "ChargeTransactionPayment" => Some(json!(0)),
        "ChargeAssetTxPayment" => Some(json!({"tip": 0, "asset_id": null})),
        // Extensions whose extra type is unit need no value
        _ => None,
    }
}

/// Returns a default JSON value for well-known extension "additional_signed" data.
fn default_additional_value(
    identifier: &str,
    ctx: &ChainContext,
) -> Option<JsonValue> {
    match identifier {
        "CheckSpecVersion" => Some(json!(ctx.spec_version)),
        "CheckTxVersion" => Some(json!(ctx.tx_version)),
        "CheckGenesis" => {
            Some(json!(format!("0x{}", hex::encode(ctx.genesis_hash))))
        }
        "CheckMortality" => {
            // Immortal era → checkpoint is genesis hash
            Some(json!(format!("0x{}", hex::encode(ctx.genesis_hash))))
        }
        _ => None,
    }
}

async fn submit<'m, V>(
    chain: impl Backend,
    meta: &'m Metadata,
    path: &str,
    tx_data: ExtrinsicBody<V>,
    signer: impl Signer,
) -> Result<Response<'m>>
where
    V: serde::Serialize + core::fmt::Debug,
{
    let (pallet, item_or_call, _keys) = parse_uri(path).ok_or(Error::BadInput)?;
    let pallet = meta
        .pallet_by_name(&pallet)
        .ok_or(Error::PalletNotFound(pallet))?;
    let calls_ty = pallet.calls_ty.ok_or(Error::CallNotFound)?;

    log::debug!("calls_ty: {:?}", calls_ty);

    let mut encoded_call = vec![pallet.index];

    log::debug!("tx_data: {:?}", tx_data);
    let json = &json!({
        &item_or_call.to_lowercase(): &tx_data.body
    });
    log::debug!("json_body: {:?}", &json);

    let call_data = scales::to_vec_with_info(&json, Some((&meta.registry, calls_ty)))
        .map_err(|e| Error::Encode(e.to_string()))?;

    encoded_call.extend(&call_data);

    let from_account = signer.account();
    log::debug!("from_account: {:?}", hex::encode(from_account.as_ref()));

    // --- Fetch chain context ---
    let ctx = {
        // Spec/tx version from System::Version constant
        let system = meta
            .pallet_by_name("System")
            .ok_or(Error::PalletNotFound(String::from("System")))?;

        let version_const = system
            .constants
            .iter()
            .find(|c| c.name == "Version")
            .ok_or(Error::ConstantNotFound("System_Version".into()))?;

        let chain_value: JsonValue = Value::new(&version_const.value, version_const.ty, &meta.registry)
            .try_into()
            .map_err(|_| Error::Mapping("failed to decode System::Version".into()))?;

        let obj = chain_value
            .as_object()
            .ok_or(Error::ConstantNotFound("System_Version".into()))?;

        let spec_version = obj
            .get("spec_version")
            .and_then(|v| v.as_u64())
            .ok_or(Error::Mapping("spec_version not found".into()))? as u32;

        let tx_version = obj
            .get("transaction_version")
            .and_then(|v| v.as_u64())
            .ok_or(Error::Mapping("transaction_version not found".into()))?
            as u32;

        let genesis_block: Vec<u8> = chain.block_info(Some(0u32)).await?.into();
        let mut genesis_hash = [0u8; 32];
        genesis_hash.copy_from_slice(&genesis_block[..32]);

        // Nonce: from caller override or query chain
        let account_nonce = if let Some(nonce) = tx_data.nonce {
            nonce
        } else if let Some(nonce_val) = find_extension_override(&tx_data.extensions, "CheckNonce") {
            nonce_val.as_u64().ok_or(Error::Mapping("CheckNonce override is not a number".into()))?
        } else {
            let response = query(
                &chain,
                meta,
                &format!("system/account/0x{}", hex::encode(from_account.as_ref())),
                None,
            )
            .await?;

            match response {
                Response::Value(entry, reg) => {
                    let value = entry.as_value(reg);
                    let nonce_val = value.field("nonce").and_then(|v| {
                        v.as_u32().map(|n| n as u64).or_else(|| v.as_u64())
                    });
                    match nonce_val {
                        Some(n) => n,
                        None => {
                            let json_val: JsonValue = value
                                .try_into()
                                .map_err(|_| Error::Mapping("failed to decode account info".into()))?;
                            json_val
                                .as_object()
                                .and_then(|o| o.get("nonce"))
                                .and_then(|v| v.as_u64())
                                .ok_or(Error::Mapping("nonce not found in account info".into()))?
                        }
                    }
                }
                Response::None => {
                    log::warn!("account not found");
                    0
                }
                _ => return Err(Error::AccountNotFound),
            }
        };

        ChainContext {
            spec_version,
            tx_version,
            genesis_hash,
            account_nonce,
        }
    };

    // --- Encode extensions by iterating metadata ---
    let mut extra_bytes = Vec::new();
    let mut additional_signed_bytes = Vec::new();

    for ext in &meta.extrinsic.extensions {
        // "extra" bytes — included in extrinsic body
        let extra_value = find_extension_override(&tx_data.extensions, &ext.identifier)
            .or_else(|| default_extra_value(&ext.identifier, &ctx));

        let encoded = if meta::is_zero_size_type(ext.ty, &meta.registry) {
            vec![]
        } else {
            let value = extra_value.ok_or_else(|| {
                Error::MissingExtensionValue(ext.identifier.clone())
            })?;
            let mut buf = vec![];
            scales::to_bytes_with_info(&mut buf, &value, Some((&meta.registry, ext.ty)))
                .map_err(|e| Error::Encode(e.to_string()))?;
            buf
        };
        extra_bytes.extend(encoded);

        // "additional_signed" bytes — signing payload only
        let additional_value = default_additional_value(&ext.identifier, &ctx);

        let encoded = if meta::is_zero_size_type(ext.additional_signed, &meta.registry) {
            vec![]
        } else {
            let value = additional_value.ok_or_else(|| {
                Error::MissingExtensionValue(format!("{} (additional_signed)", ext.identifier))
            })?;
            let mut buf = vec![];
            scales::to_bytes_with_info(
                &mut buf,
                &value,
                Some((&meta.registry, ext.additional_signed)),
            )
            .map_err(|e| Error::Encode(e.to_string()))?;
            buf
        };
        additional_signed_bytes.extend(encoded);
    }

    // --- Sign ---
    let signature_payload = [
        encoded_call.clone(),
        extra_bytes.clone(),
        additional_signed_bytes,
    ]
    .concat();

    let payload = if signature_payload.len() > 256 {
        hash(&meta::Hasher::Blake2_256, &signature_payload[..])
    } else {
        signature_payload
    };

    let signature = signer.sign(payload).await?;

    // --- Assemble extrinsic ---
    let version = meta.extrinsic.version;

    // Address encoding
    let address_bytes = if meta.extrinsic.address_ty.is_some() {
        // V15+: For now, MultiAddress::Id is variant 0 followed by 32-byte account
        [vec![0x00], from_account.as_ref().to_vec()].concat()
    } else {
        // V14 fallback
        [vec![0x00], from_account.as_ref().to_vec()].concat()
    };

    // Signature prefix: find Sr25519 variant index from signature_ty
    let sig_prefix = if let Some(sig_ty) = meta.extrinsic.signature_ty {
        match meta.registry.resolve(sig_ty) {
            Some(scales::TypeDef::Variant(vdef)) => {
                // Find the Sr25519 variant
                vdef.variants
                    .iter()
                    .find(|v| v.name.contains("Sr25519"))
                    .map(|v| v.index)
                    .unwrap_or(1) // fallback to 1 if not found
            }
            _ => 0x01, // fallback
        }
    } else {
        0x01 // V14 fallback: Sr25519
    };

    let extrinsic_call = {
        let encoded_inner = [
            // header: "is signed" flag | version
            vec![0b10000000 | version],
            // address
            address_bytes,
            // signature
            [vec![sig_prefix], signature.as_ref().to_vec()].concat(),
            // extra (extension data)
            extra_bytes,
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

    /// Borrow this entry as a typed [`Value`] for decoding/serialization.
    pub fn as_value<'a>(&'a self, registry: &'a scales::Registry) -> scales::Value<'a> {
        scales::Value::new(&self.data, self.ty, registry)
    }

    /// Decode this entry into a JSON value using the given registry.
    pub fn to_json(&self, registry: &scales::Registry) -> Result<JsonValue> {
        serde_json::to_value(self.as_value(registry))
            .map_err(|e| Error::Mapping(e.to_string()))
    }
}

#[derive(Debug)]
pub enum Response<'m> {
    Void,
    None,
    Value(StorageEntry, &'m scales::Registry),
    ValueSet(Vec<(Vec<StorageEntry>, Option<StorageEntry>)>, &'m scales::Registry),
    Meta(&'m Metadata),
    Registry(&'m scales::Registry),
}

impl From<Response<'_>> for Vec<u8> {
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

fn parse_uri(uri: &str) -> Option<(String, String, Vec<String>)> {
    let mut path = uri.trim_matches('/').split('/');
    let pallet = path.next().map(to_camel)?;
    let item = path.next().map(to_camel)?;
    let map_keys = path.map(to_camel).collect::<Vec<_>>();
    Some((pallet, item, map_keys))
}

#[derive(Deserialize, Serialize, Debug)]
pub struct StorageChangeSet {
    block: String,
    changes: Vec<(String, Option<String>)>,
}

pub type RawKey = Vec<u8>;
pub type RawValue = Vec<u8>;

/// Generic definition of a blockchain backend
///
/// ```rust,ignore
/// pub trait Backend {
///     async fn query_bytes(&self, key: &StorageKey) -> Result<Vec<u8>>;
///
///     async fn submit<T>(&self, ext: T) -> Result<()>
///     where
///         T: AsRef<[u8]>;
///
///     async fn metadata(&self) -> Result<Metadata>;
/// }
/// ```
pub trait Backend {
    async fn get_storage_items(
        &self,
        keys: Vec<RawKey>,
        block: Option<u32>,
    ) -> crate::Result<impl Iterator<Item = (RawKey, Option<RawValue>)>>;

    async fn get_storage_item(
        &self,
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
        &self,
        from: RawKey,
        size: u16,
        to: Option<RawKey>,
    ) -> crate::Result<Vec<RawValue>>;

    /// Send a signed extrinsic to the blockchain
    async fn submit(&self, ext: impl AsRef<[u8]>) -> Result<()>;

    async fn metadata(&self) -> Result<Metadata>;

    async fn block_info(&self, at: Option<u32>) -> Result<meta::BlockInfo>;
}

/// A Dummy backend for offline querying of metadata
pub struct Offline(pub Metadata);

impl Backend for Offline {
    async fn get_storage_items(
        &self,
        _keys: Vec<RawKey>,
        _block: Option<u32>,
    ) -> crate::Result<impl Iterator<Item = (RawKey, Option<RawValue>)>> {
        Err::<Empty<(RawKey, Option<RawValue>)>, _>(Error::ChainUnavailable)
    }

    async fn get_keys_paged(
        &self,
        _from: RawKey,
        _size: u16,
        _to: Option<RawKey>,
    ) -> crate::Result<Vec<RawKey>> {
        Err(Error::ChainUnavailable)
    }

    /// Send a signed extrinsic to the blockchain
    async fn submit(&self, _ext: impl AsRef<[u8]>) -> Result<()> {
        Err(Error::ChainUnavailable)
    }

    async fn metadata(&self) -> Result<Metadata> {
        Ok(self.0.clone())
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
    CantInitBackend,
    CantDecodeReponseForMeta,
    CantDecodeRawQueryResponse,
    CantFindMethodInPallet,
    BadBlockNumber,
    MissingExtensionValue(String),
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
        let version = system.constants.iter().find(|c| c.name == "Version").unwrap();
        let entry = StorageEntry::new(version.value.clone(), version.ty);
        let json = entry.to_json(&meta.registry).expect("decodes to JSON");
        assert!(json.get("spec_name").is_some());
    }
}
