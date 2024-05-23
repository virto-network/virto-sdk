use crate::prelude::*;
use core::{any::Any, borrow::Borrow, slice};

use codec::{Decode, Encode};

#[cfg(any(feature = "v14"))]
use frame_metadata::v14::PalletCallMetadata;
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};

use scale_info::{form::PortableForm, Type, TypeInfo};
use scales::to_bytes_with_info;

#[cfg(feature = "v14")]
pub use v14::*;

use crate::hasher::hash;
type TypeId = u32;

#[cfg(feature = "v14")]
mod v14 {
    use frame_metadata::v14::*;
    use scale_info::form::PortableForm;
    pub type Metadata = RuntimeMetadataV14;
    pub type ExtrinsicMeta = ExtrinsicMetadata;
    pub type PalletMeta = PalletMetadata<PortableForm>;
    pub type CallsMeta = PalletCallMetadata<PortableForm>;
    pub type StorageMeta = PalletStorageMetadata<PortableForm>;
    pub type ConstMeta = PalletConstantMetadata<PortableForm>;
    pub type EntryMeta = StorageEntryMetadata<PortableForm>;
    pub type EntryType = StorageEntryType<PortableForm>;
    pub type Hasher = StorageHasher;
    pub use scale_info::PortableRegistry;
    pub type Type = scale_info::Type<PortableForm>;
    pub type TypeDef = scale_info::TypeDef<PortableForm>;
}

/// Decode metadata from its raw prefixed format to the currently
/// active metadata version.
pub fn from_bytes(bytes: &mut &[u8]) -> core::result::Result<Metadata, codec::Error> {
    let meta: RuntimeMetadataPrefixed = Decode::decode(bytes)?;
    let meta = match meta.1 {
        #[cfg(feature = "v14")]
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

impl Into<Vec<u8>> for BlockInfo {
    fn into(self) -> Vec<u8> {
        self.hash.into()
    }
}

type Entries<'a, E> = slice::Iter<'a, E>;

/// An extension trait for a decoded metadata object that provides
/// convenient methods to navigate and extract data from it.
pub trait Meta<'a> {
    type Pallet: Pallet<'a>;

    fn pallets(&self) -> Pallets<Self::Pallet>;

    fn pallet_by_name(&self, name: &str) -> Option<&Self::Pallet> {
        self.pallets()
            .find(|p| p.name().to_lowercase() == name.to_lowercase())
    }
    fn cloned(&self) -> Self;
}

#[cfg(feature = "v14")]
pub trait Registry {
    fn find_ids(&self, q: &str) -> Vec<u32>;
    fn find(&self, q: &str) -> Vec<&scale_info::Type<scale_info::form::PortableForm>>;
}

type Pallets<'a, P> = slice::Iter<'a, P>;

pub trait Pallet<'a> {
    type Storage: Storage<'a>;

    fn name(&self) -> &str;
    fn storage(&self) -> Option<&Self::Storage>;
    fn calls(&self) -> Option<&CallsMeta>;
}

pub trait Storage<'a> {
    type Entry: Entry + 'a;

    fn module(&self) -> &str;
    fn entries(&'a self) -> Entries<'a, Self::Entry>;

    fn entry(&'a self, name: &str) -> Option<&'a Self::Entry> {
        self.entries().find(|e| e.name() == name)
    }
}

#[derive(Clone, Debug)]
pub enum KeyValue {
    Empty(TypeId),
    /*
     * // type id, hash, encoded_value
     */
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
        !self
            .args
            .iter()
            .fold(true, |p, n| p && matches!(n, KeyValue::Value(_)))
    }

    pub fn build_with_registry<T: AsRef<str>>(
        registry: &PortableRegistry,
        meta: &PalletMeta,
        item: &str,
        map_keys: &[T],
    ) -> crate::Result<Self> {
        let entry = meta
            .storage()
            .map(|s| s.entries().find(|e| e.name() == item))
            .flatten()
            .ok_or(crate::Error::StorageKeyNotFound)?;

        Ok(entry.key(&registry, meta.name(), map_keys)?)
    }
}

impl core::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "0x{}", hex::encode(self.key()))
    }
}

pub trait Entry {
    type Type: EntryTy;

    fn name(&self) -> &str;
    fn ty(&self) -> &Self::Type;
    fn ty_id(&self) -> u32;

    fn key<T: AsRef<str>>(
        &self,
        registry: &PortableRegistry,
        pallet: &str,
        map_keys: &[T],
    ) -> crate::Result<StorageKey> {
        self.ty().key(&registry, pallet, self.name(), map_keys)
    }
}

fn extract_touple_type(key_id: u32, type_info: &Type<PortableForm>) -> Vec<TypeId> {
    match &type_info.type_def {
        scale_info::TypeDef::Tuple(touple) => {
            let types = touple.fields.iter().map(|x| x.id).collect();
            return types;
        }
        scale_info::TypeDef::Primitive(t) => vec![key_id],
        _ => vec![],
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
        key_ty_id: u32,
        value_ty_id: u32,
        pallet: &str,
        item: &str,
        map_keys: &[T],
        hashers: &[H],
    ) -> crate::Result<StorageKey>
    where
        H: Borrow<Hasher>,
        T: AsRef<str>,
    {
        let key_type_info = portable_reg
            .resolve(key_ty_id)
            .ok_or(crate::Error::BadInput)?;
        let type_call_ids = extract_touple_type(key_ty_id, key_type_info);
        let storage_key = StorageKey::new(
            value_ty_id,
            hash(&Hasher::Twox128, &pallet),
            hash(&Hasher::Twox128, &item),
            type_call_ids
                .into_iter()
                .enumerate()
                .map(|(i, type_id)| {
                    let k = map_keys.get(i);
                    let hasher = hashers.get(i).expect("hasher not found");

                    if k.is_none() {
                        return KeyValue::Empty(type_id);
                    }

                    let k = k.expect("it must exist").as_ref();

                    let hasher = hasher.borrow();
                    let mut out = vec![];
                    let key_type = portable_reg
                        .resolve(type_id)
                        .expect("type can not be resolved");

                    if k.starts_with("0x") {
                        let value = hex::decode(&k[2..]).expect("str must be encoded");
                        to_bytes_with_info(&mut out, &value, Some((&portable_reg, type_id)));
                    } else {
                        to_bytes_with_info(&mut out, &k, Some((&portable_reg, type_id)));
                    }

                    let hash = hash(hasher, &out);
                    KeyValue::Value((type_id, hash, out, hasher.clone()))
                })
                .collect(),
        );

        Ok(storage_key)
    }
}

impl<'a> Meta<'a> for Metadata {
    type Pallet = PalletMeta;
    #[cfg(feature = "v14")]
    fn pallets(&self) -> Pallets<Self::Pallet> {
        self.pallets.iter()
    }
    fn cloned(&self) -> Self {
        #[cfg(feature = "v14")]
        let meta = self.clone();
        meta
    }
}

#[cfg(feature = "v14")]
impl Registry for scale_info::PortableRegistry {
    fn find_ids(&self, path: &str) -> Vec<u32> {
        self.types()
            .iter()
            .filter(|t| {
                t.ty()
                    .path()
                    .segments()
                    .iter()
                    .any(|s| s.to_lowercase().contains(&path.to_lowercase()))
            })
            .map(|t| t.id())
            .collect()
    }
    fn find(&self, path: &str) -> Vec<&scale_info::Type<scale_info::form::PortableForm>> {
        self.find_ids(path)
            .into_iter()
            .map(|t| self.resolve(t).unwrap())
            .collect()
    }
}

impl<'a> Pallet<'a> for PalletMeta {
    type Storage = StorageMeta;
    fn storage(&self) -> Option<&Self::Storage> {
        let storage = self.storage.as_ref();
        #[cfg(not(feature = "v14"))]
        let storage = storage.map(|s| s.decoded());
        storage
    }

    fn name(&self) -> &str {
        #[cfg(feature = "v14")]
        let name = self.name.as_ref();
        name
    }

    fn calls(&self) -> Option<&CallsMeta> {
        self.calls.as_ref()
    }
}

impl<'a> Storage<'a> for StorageMeta {
    type Entry = EntryMeta;

    fn module(&self) -> &str {
        #[cfg(feature = "v14")]
        let pref = self.prefix.as_ref();
        pref
    }

    fn entries(&'a self) -> Entries<Self::Entry> {
        #[cfg(feature = "v14")]
        let entries = self.entries.iter();
        entries
    }
}

impl<'a> Entry for EntryMeta {
    type Type = EntryType;

    fn name(&self) -> &str {
        #[cfg(feature = "v14")]
        let name = self.name.as_ref();
        name
    }
    fn ty(&self) -> &Self::Type {
        &self.ty
    }

    fn ty_id(&self) -> u32 {
        #[cfg(feature = "v14")]
        match &self.ty {
            EntryType::Plain(t) => t.id(),
            EntryType::Map { value, .. } => value.id(),
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
                self.build_call::<Hasher, &str>(registry, 0, ty.id, pallet, item, &[], &[])
            }
            #[cfg(feature = "v14")]
            Self::Map {
                hashers,
                key,
                value,
            } => {
                log::debug!("Item Type[{}]: {:?} => {:?}", key.id, key, value);
                self.build_call(&registry, key.id, value.id, pallet, item, map_keys, hashers)
            }
        }
    }
}
