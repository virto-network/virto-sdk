use crate::prelude::*;
use core::borrow::Borrow;

use codec::Decode;
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};
use scales::to_bytes_with_info;

#[cfg(feature = "v14")]
pub use v14::*;

use crate::hasher::hash;
type TypeId = u32;

mod v14 {
    use frame_metadata::v14::*;
    use scale_info::form::PortableForm;
    pub type Metadata = RuntimeMetadataV14;
    pub type PalletMeta = PalletMetadata<PortableForm>;
    pub type EntryType = StorageEntryType<PortableForm>;
    pub type Hasher = StorageHasher;
    pub use scale_info::PortableRegistry;
    pub type Type = scale_info::Type<PortableForm>;
}

// Decode metadata from its raw prefixed format to the currently
// active metadata version.
pub fn from_bytes(bytes: &mut &[u8]) -> core::result::Result<Metadata, codec::Error> {
    let meta: RuntimeMetadataPrefixed = Decode::decode(bytes)?;
    let meta = match meta.1 {
        RuntimeMetadata::V14(m) => m,
        _ => unreachable!("Metadata version not supported"),
    };
    Ok(meta)
}

pub struct BlockInfo {
    pub number: u64,
    pub hash: [u8; 32],
    pub parent: [u8; 32],
}
impl From<BlockInfo> for Vec<u8> {
    fn from(b: BlockInfo) -> Self {
        b.hash.into()
    }
}

/// An extension trait for a decoded metadata object that provides
/// convenient methods to navigate and extract data from it.
pub trait Meta {
    type Pallet: Pallet;

    fn pallets(&self) -> impl Iterator<Item = &Self::Pallet>;

    fn pallet_by_name(&self, name: &str) -> Option<&Self::Pallet> {
        self.pallets()
            .find(|p| p.name().to_lowercase() == name.to_lowercase())
    }
}
impl Meta for Metadata {
    type Pallet = PalletMeta;

    fn pallets(&self) -> impl Iterator<Item = &Self::Pallet> {
        self.pallets.iter()
    }
}

pub trait Pallet {
    fn name(&self) -> &str;
}
impl Pallet for PalletMeta {
    fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug)]
pub enum KeyValue {
    Empty(TypeId),
    // type id, hash, encoded_value
    Value((TypeId, Vec<u8>, Vec<u8>, Hasher)),
}

/// Represents a key of the blockchain storage in its raw form
#[derive(Clone, Debug)]
pub struct StorageKey {
    pub pallet: Vec<u8>,
    pub call: Vec<u8>,
    pub args: Vec<KeyValue>,
    pub ty: u32,
}

impl StorageKey {
    pub fn new(ty: u32, pallet: Vec<u8>, call: Vec<u8>, args: Vec<KeyValue>) -> Self {
        Self {
            ty,
            pallet,
            call,
            args,
        }
    }

    pub fn key(&self) -> Vec<u8> {
        let args = self
            .args
            .iter()
            .map(|e| match e {
                KeyValue::Empty(_) => &[],
                KeyValue::Value((_, hash, _, _)) => &hash[..],
            })
            .collect::<Vec<&[u8]>>()
            .concat();

        [&self.pallet[..], &self.call[..], &args[..]].concat()
    }

    pub fn is_partial(&self) -> bool {
        !self.args.iter().all(|n| matches!(n, KeyValue::Value(_)))
    }

    pub fn build_with_registry<T: AsRef<str>>(
        registry: &PortableRegistry,
        meta: &PalletMeta,
        item: &str,
        map_keys: &[T],
    ) -> crate::Result<Self> {
        let entry = meta
            .storage
            .as_ref()
            .and_then(|s| s.entries.iter().find(|e| e.name == item))
            .ok_or(crate::Error::CantFindMethodInPallet)?;
        log::trace!(
            "map_keys={}",
            map_keys
                .iter()
                .map(|x| x.as_ref())
                .collect::<Vec<&str>>()
                .join(", ")
        );
        entry.ty.key(registry, &meta.name, &entry.name, map_keys)
    }
}

impl core::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "0x{}", hex::encode(self.key()))
    }
}

fn extract_touple_type(key_id: u32, type_info: &Type) -> Vec<TypeId> {
    match &type_info.type_def {
        scale_info::TypeDef::Tuple(touple) => {
            let types = touple.fields.iter().map(|x| x.id).collect();
            types
        }
        _ => vec![key_id],
    }
}

pub trait EntryTy {
    fn key<T: AsRef<str>>(
        &self,
        registry: &PortableRegistry,
        pallet: &str,
        item: &str,
        map_keys: &[T],
    ) -> crate::Result<StorageKey>;

    fn build_call<H, T>(
        &self,
        portable_reg: &PortableRegistry,
        key_ty_id: Option<u32>,
        value_ty_id: u32,
        pallet_item: (&str, &str),
        map_keys: &[T],
        hashers: &[H],
    ) -> crate::Result<StorageKey>
    where
        H: Borrow<Hasher>,
        T: AsRef<str>,
    {
        let type_call_ids = if let Some(key_ty_id) = key_ty_id {
            let key_type_info = portable_reg
                .resolve(key_ty_id)
                .ok_or(crate::Error::BadInput)?;

            log::trace!("key_type_info={:?}", key_type_info);
            extract_touple_type(key_ty_id, key_type_info)
        } else {
            vec![]
        };

        if type_call_ids.len() == hashers.len() {
            log::trace!("type_call_ids={:?}", type_call_ids);
            let storage_key = StorageKey::new(
                value_ty_id,
                hash(&Hasher::Twox128, pallet_item.0),
                hash(&Hasher::Twox128, pallet_item.1),
                type_call_ids
                    .into_iter()
                    .enumerate()
                    .map(|(i, type_id)| {
                        log::trace!("type_call_ids.i={} type_call_ids.type_id={}", i, type_id);
                        let k = map_keys.get(i);
                        let hasher = hashers.get(i).expect("hasher not found");

                        if k.is_none() {
                            return KeyValue::Empty(type_id);
                        }

                        let k = k.expect("it must exist").as_ref();

                        let hasher = hasher.borrow();
                        let mut out = vec![];

                        if let Some(k) = k.strip_prefix("0x") {
                            let value = hex::decode(k).expect("str must be encoded");
                            let _ =
                                to_bytes_with_info(&mut out, &value, Some((portable_reg, type_id)));
                        } else {
                            let _ = to_bytes_with_info(&mut out, &k, Some((portable_reg, type_id)));
                        }

                        let hash = hash(hasher, &out);
                        KeyValue::Value((type_id, hash, out, hasher.clone()))
                    })
                    .collect(),
            );

            Ok(storage_key)
        } else if hashers.len() == 1 {
            log::trace!("treating touple as argument for hasher");

            let touple_hex: Vec<u8> = type_call_ids
                .into_iter()
                .enumerate()
                .flat_map(|(i, type_id)| {
                    let k = map_keys.get(i).expect("to exist in map_keys").as_ref();
                    let mut out = vec![];
                    if let Some(k) = k.strip_prefix("0x") {
                        let value = hex::decode(k).expect("str must be hex encoded");
                        let _ = to_bytes_with_info(&mut out, &value, Some((portable_reg, type_id)));
                    } else {
                        let _ = to_bytes_with_info(&mut out, &k, Some((portable_reg, type_id)));
                    }
                    out
                })
                .collect();

            let hasher = hashers.first().expect("hasher not found");
            let hasher = hasher.borrow();
            let hashed_value = hash(hasher, &touple_hex);

            let storage_key = StorageKey::new(
                value_ty_id,
                hash(&Hasher::Twox128, pallet_item.0),
                hash(&Hasher::Twox128, pallet_item.1),
                vec![KeyValue::Value((
                    key_ty_id.expect("must key id must work"),
                    hashed_value,
                    touple_hex,
                    hasher.clone(),
                ))],
            );

            Ok(storage_key)
        } else {
            Err(crate::Error::Encode(
                "Wrong number of hashers vs map_keys".into(),
            ))
        }
    }
}

impl EntryTy for EntryType {
    fn key<T: AsRef<str>>(
        &self,
        registry: &PortableRegistry,
        pallet: &str,
        item: &str,
        map_keys: &[T],
    ) -> crate::Result<StorageKey> {
        match self {
            Self::Plain(ty) => {
                self.build_call::<Hasher, &str>(registry, None, ty.id, (pallet, item), &[], &[])
            }
            Self::Map {
                hashers,
                key,
                value,
            } => {
                log::trace!("key={}, value={}, hasher={:?}", key.id, value.id, hashers);
                self.build_call(
                    registry,
                    Some(key.id),
                    value.id,
                    (pallet, item),
                    map_keys,
                    hashers,
                )
            }
        }
    }
}
