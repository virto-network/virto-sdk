use core::slice;

use codec::Decode;
#[cfg(any(feature = "v12", feature = "v13"))]
use frame_metadata::decode_different::DecodeDifferent;
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};

#[cfg(feature = "v12")]
pub use v12::*;
#[cfg(feature = "v13")]
pub use v13::*;
#[cfg(feature = "v14")]
pub use v14::*;

use crate::hasher::hash;

#[cfg(feature = "v12")]
mod v12 {
    use frame_metadata::v12::*;
    pub type Metadata = RuntimeMetadataV12;
    pub type PalletMeta = ModuleMetadata;
    pub type StorageMeta = StorageMetadata;
    pub type EntryMeta = StorageEntryMetadata;
    pub type EntryType = StorageEntryType;
    pub type Hasher = StorageHasher;
}
#[cfg(feature = "v13")]
mod v13 {
    use frame_metadata::v13::*;
    pub type Metadata = RuntimeMetadataV13;
    pub type PalletMeta = ModuleMetadata;
    pub type StorageMeta = StorageMetadata;
    pub type EntryMeta = StorageEntryMetadata;
    pub type EntryType = StorageEntryType;
    pub type Hasher = StorageHasher;
}
#[cfg(feature = "v14")]
mod v14 {
    use frame_metadata::v14::*;
    use scale_info::form::PortableForm;
    pub type Metadata = RuntimeMetadataV14;
    pub type PalletMeta = PalletMetadata<PortableForm>;
    pub type StorageMeta = PalletStorageMetadata<PortableForm>;
    pub type EntryMeta = StorageEntryMetadata<PortableForm>;
    pub type EntryType = StorageEntryType<PortableForm>;
    pub type Hasher = StorageHasher;
}

/// Decode metadata from its raw prefixed format to the currently
/// active metadata version.
pub fn meta_from_bytes(bytes: &mut &[u8]) -> Result<Metadata, codec::Error> {
    let meta: RuntimeMetadataPrefixed = Decode::decode(bytes)?;
    let meta = match meta.1 {
        #[cfg(feature = "v12")]
        RuntimeMetadata::V12(m) => m,
        #[cfg(feature = "v13")]
        RuntimeMetadata::V13(m) => m,
        #[cfg(feature = "v14")]
        RuntimeMetadata::V14(m) => m,
        _ => unreachable!(),
    };
    Ok(meta)
}

type EntryFor<'a, M> = <<<M as Meta<'a>>::Pallet as Pallet<'a>>::Storage as Storage<'a>>::Entry;

/// An extension trait for a decoded metadata object that provides
/// convenient methods to navigate and extract data from it.
pub trait Meta<'a> {
    type Pallet: Pallet<'a>;

    fn pallets(&self) -> Pallets<Self::Pallet>;

    fn pallet_by_name(&self, name: &str) -> Option<&Self::Pallet> {
        self.pallets().find(|p| p.name() == name)
    }

    fn storage_entries(&'a self, pallet: &str) -> Option<Entries<EntryFor<Self>>> {
        Some(self.pallet_by_name(pallet)?.storage()?.entries())
    }

    fn storage_entry(&'a self, pallet: &str, entry: &str) -> Option<&EntryFor<Self>> {
        self.storage_entries(pallet)?.find(|e| e.name() == entry)
    }
}
type Pallets<'a, P> = slice::Iter<'a, P>;

pub trait Pallet<'a> {
    type Storage: Storage<'a>;

    fn name(&self) -> &str;
    fn storage(&self) -> Option<&Self::Storage>;
}

pub trait Storage<'a> {
    type Entry: Entry + 'a;

    fn module(&self) -> &str;
    fn entries(&'a self) -> Entries<'a, Self::Entry>;

    fn entry(&'a self, name: &str) -> Option<&'a Self::Entry> {
        self.entries().find(|e| e.name() == name)
    }
}
type Entries<'a, E> = slice::Iter<'a, E>;

pub trait Entry {
    type Type: EntryTy;

    fn name(&self) -> &str;
    fn ty(&self) -> &Self::Type;

    fn key(&self, pallet: &str, map_keys: &[&str]) -> Option<Vec<u8>> {
        self.ty().key(pallet, self.name(), map_keys)
    }
}

pub trait EntryTy {
    fn key(&self, pallet: &str, item: &str, map_keys: &[&str]) -> Option<Vec<u8>>;
}

impl<'a> Meta<'a> for Metadata {
    type Pallet = PalletMeta;
    #[cfg(not(feature = "v14"))]
    fn pallets(&self) -> Pallets<Self::Pallet> {
        self.modules.decoded().iter()
    }
    #[cfg(feature = "v14")]
    fn pallets(&self) -> Pallets<Self::Pallet> {
        self.pallets.iter()
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
        #[cfg(not(feature = "v14"))]
        let name = self.name.decoded();
        name
    }
}

impl<'a> Storage<'a> for StorageMeta {
    type Entry = EntryMeta;

    fn module(&self) -> &str {
        #[cfg(feature = "v14")]
        let pref = self.prefix.as_ref();
        #[cfg(not(feature = "v14"))]
        let pref = self.prefix.decoded();
        pref
    }

    fn entries(&'a self) -> Entries<Self::Entry> {
        #[cfg(feature = "v14")]
        let entries = self.entries.iter();
        #[cfg(not(feature = "v14"))]
        let entries = self.entries.decoded().iter();
        entries
    }
}

impl<'a> Entry for EntryMeta {
    type Type = EntryType;

    fn name(&self) -> &str {
        #[cfg(feature = "v14")]
        let name = self.name.as_ref();
        #[cfg(not(feature = "v14"))]
        let name = self.name.decoded();
        name
    }
    fn ty(&self) -> &Self::Type {
        &self.ty
    }
}

impl EntryTy for EntryType {
    fn key(&self, pallet: &str, item: &str, map_keys: &[&str]) -> Option<Vec<u8>> {
        let mut key = hash(&Hasher::Twox128, &pallet);
        key.append(&mut hash(&Hasher::Twox128, &item));

        Some(match self {
            Self::Plain(_) => key,
            Self::Map { ref hasher, .. } => {
                if map_keys.len() != 1 || map_keys[0].is_empty() {
                    return None;
                }
                let k1 = map_keys[0];
                key.append(&mut hash(hasher, &k1));
                key
            }
            Self::DoubleMap {
                hasher,
                key2_hasher,
                ..
            } => {
                if map_keys.len() != 2 {
                    return None;
                }
                let k1 = map_keys[0];
                let k2 = map_keys[1];
                if k1.is_empty() || k2.is_empty() {
                    return None;
                }
                key.append(&mut hash(hasher, &k1));
                key.append(&mut hash(key2_hasher, &k2));
                key
            }
            #[cfg(not(feature = "v12"))]
            Self::NMap { hashers, .. } => {
                #[cfg(feature = "v13")]
                let hashers = hashers.decoded();
                if map_keys.len() != hashers.len() {
                    return None;
                }

                for (i, h) in hashers.iter().enumerate() {
                    key.append(&mut hash(h, map_keys[i]))
                }
                key
            }
        })
    }
}

#[cfg(any(feature = "v12", feature = "v13"))]
trait Decoded {
    type Output;
    fn decoded(&self) -> &Self::Output;
}

#[cfg(any(feature = "v12", feature = "v13"))]
impl<B, O> Decoded for DecodeDifferent<B, O> {
    type Output = O;
    fn decoded(&self) -> &Self::Output {
        match self {
            DecodeDifferent::Decoded(o) => o,
            _ => unreachable!(),
        }
    }
}
