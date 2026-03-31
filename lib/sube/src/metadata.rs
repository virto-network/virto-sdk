use crate::prelude::*;

use scales::to_bytes_with_info;

use crate::hasher::hash;

/// Encode a key value from its text representation into SCALE bytes.
///
/// Tries `from_text` first (handles complex types like tuples, variants, structs).
/// Falls back to hex decoding for `0x`-prefixed values, then to serde serialization.
fn encode_key(key: &str, registry: &scales::Registry, ty_id: TypeId) -> Vec<u8> {
    // Try the text format first — handles everything including complex types
    if let Ok(bytes) = scales::from_text(key, registry, ty_id) {
        return bytes;
    }

    // Fallback: hex-prefixed raw bytes or plain string via serde
    let mut out = vec![];
    if let Some(hex_str) = key.strip_prefix("0x") {
        if let Ok(value) = hex::decode(hex_str) {
            let _ = to_bytes_with_info(&mut out, &value, Some((registry, ty_id)));
        }
    } else {
        let _ = to_bytes_with_info(&mut out, &key, Some((registry, ty_id)));
    }
    out
}

pub type TypeId = u32;

/// Storage hasher types used by Substrate.
#[derive(Clone, Debug)]
pub enum Hasher {
    Blake2_128,
    Blake2_256,
    Blake2_128Concat,
    Twox128,
    Twox256,
    Twox64Concat,
    Identity,
}

impl Hasher {
    pub fn key_prefix_len(&self) -> usize {
        match self {
            Hasher::Blake2_128Concat => 16,
            Hasher::Twox64Concat => 8,
            Hasher::Identity => 0,
            Hasher::Blake2_128 | Hasher::Blake2_256 | Hasher::Twox128 | Hasher::Twox256 => 0,
        }
    }

    pub fn is_transparent(&self) -> bool {
        matches!(
            self,
            Hasher::Blake2_128Concat | Hasher::Twox64Concat | Hasher::Identity
        )
    }

    /// Convert from SCALE enum index (as used in metadata wire format).
    fn from_scale_index(idx: u8) -> Self {
        match idx {
            0 => Hasher::Blake2_128,
            1 => Hasher::Blake2_256,
            2 => Hasher::Blake2_128Concat,
            3 => Hasher::Twox128,
            4 => Hasher::Twox256,
            5 => Hasher::Twox64Concat,
            _ => Hasher::Identity, // 6 = Identity, default to Identity for unknown
        }
    }
}

/// Compressed constant metadata — only name, type id, and raw value.
#[derive(Clone, Debug)]
pub struct ConstantMeta {
    pub name: String,
    pub ty: TypeId,
    pub value: Vec<u8>,
}

/// Storage entry type — plain value or map with hashers.
#[derive(Clone, Debug)]
pub enum StorageEntryType {
    Plain(TypeId),
    Map {
        hashers: Vec<Hasher>,
        key: TypeId,
        value: TypeId,
    },
}

/// Compressed storage entry metadata — only name and type info.
#[derive(Clone, Debug)]
pub struct StorageEntryMeta {
    pub name: String,
    pub ty: StorageEntryType,
}

/// Compressed pallet storage metadata.
#[derive(Clone, Debug)]
pub struct StorageMeta {
    pub prefix: String,
    pub entries: Vec<StorageEntryMeta>,
}

/// Compressed pallet metadata — only fields sube needs at runtime.
#[derive(Clone, Debug)]
pub struct PalletMeta {
    pub name: String,
    pub index: u8,
    pub calls_ty: Option<TypeId>,
    pub storage: Option<StorageMeta>,
    pub constants: Vec<ConstantMeta>,
}

/// Metadata for a single signed/transaction extension.
#[derive(Clone, Debug)]
pub struct SignedExtensionMeta {
    pub identifier: String,
    /// Type of data included in the extrinsic body ("extra").
    pub ty: TypeId,
    /// Type of data included only in the signing payload.
    pub additional_signed: TypeId,
}

/// Extrinsic metadata extracted from the runtime.
#[derive(Clone, Debug)]
pub struct ExtrinsicMeta {
    pub version: u8,
    /// Address type — available in V15+.
    pub address_ty: Option<TypeId>,
    /// Signature type — available in V15+.
    pub signature_ty: Option<TypeId>,
    pub extensions: Vec<SignedExtensionMeta>,
}

/// Compressed runtime metadata.
#[derive(Clone, Debug)]
pub struct Metadata {
    pub pallets: Vec<PalletMeta>,
    pub extrinsic: ExtrinsicMeta,
    pub registry: scales::Registry,
}

impl Metadata {
    /// Create an empty metadata (no pallets, empty registry).
    /// Used when connecting without metadata on memory-constrained devices.
    pub fn empty() -> Self {
        Self {
            pallets: Vec::new(),
            extrinsic: ExtrinsicMeta {
                version: 0,
                address_ty: None,
                signature_ty: None,
                extensions: Vec::new(),
            },
            registry: scales::Registry::new(Vec::new()),
        }
    }
}

// Note: Registry does not implement Serialize (arena-backed, internal types are private)

// --- Decode from raw SCALE bytes using the lean decoder ---

use scales::frame::metadata as lean;

fn convert_raw_pallet(raw: lean::RawPallet) -> PalletMeta {
    PalletMeta {
        name: raw.name,
        index: raw.index,
        calls_ty: raw.calls_ty,
        storage: raw.storage.map(|s| StorageMeta {
            prefix: s.prefix,
            entries: s
                .entries
                .into_iter()
                .map(|e| StorageEntryMeta {
                    name: e.name,
                    ty: match e.ty {
                        lean::RawStorageType::Plain(t) => StorageEntryType::Plain(t),
                        lean::RawStorageType::Map {
                            hashers,
                            key,
                            value,
                        } => StorageEntryType::Map {
                            hashers: hashers.into_iter().map(Hasher::from_scale_index).collect(),
                            key,
                            value,
                        },
                    },
                })
                .collect(),
        }),
        constants: raw
            .constants
            .into_iter()
            .map(|c| ConstantMeta {
                name: c.name,
                ty: c.ty,
                value: c.value,
            })
            .collect(),
    }
}

fn convert_raw_extrinsic(raw: lean::RawExtrinsic) -> ExtrinsicMeta {
    ExtrinsicMeta {
        version: raw.version,
        address_ty: raw.address_ty,
        signature_ty: raw.signature_ty,
        extensions: raw
            .extensions
            .into_iter()
            .map(|e| SignedExtensionMeta {
                identifier: e.identifier,
                ty: e.ty,
                additional_signed: e.additional_signed,
            })
            .collect(),
    }
}

/// Decode metadata from raw SCALE bytes.
///
/// Uses the lean decoder — no `frame-metadata` or `scale-info` at runtime.
pub fn from_bytes(bytes: &[u8]) -> crate::Result<Metadata> {
    let raw = lean::decode_metadata(bytes).map_err(|_| crate::Error::BadMetadata)?;
    Ok(Metadata {
        pallets: raw.pallets.into_iter().map(convert_raw_pallet).collect(),
        extrinsic: convert_raw_extrinsic(raw.extrinsic),
        registry: scales::Registry::new(raw.types),
    })
}

/// Decode metadata keeping only the specified pallets and their referenced types.
///
/// Much smaller result for memory-constrained targets. System pallet is
/// always included.
pub fn from_bytes_filtered(bytes: &[u8], pallet_filter: &[&str]) -> crate::Result<Metadata> {
    let raw = lean::decode_metadata_filtered(bytes, pallet_filter)
        .map_err(|_| crate::Error::BadMetadata)?;
    Ok(Metadata {
        pallets: raw.pallets.into_iter().map(convert_raw_pallet).collect(),
        extrinsic: convert_raw_extrinsic(raw.extrinsic),
        registry: scales::Registry::new(raw.types),
    })
}

/// Build Metadata from pre-processed raw parts.
///
/// Used by the streaming metadata path where pallets and types are
/// decoded in separate passes.
pub fn from_raw(
    pallets: Vec<lean::RawPallet>,
    extrinsic: lean::RawExtrinsic,
    registry: scales::Registry,
) -> Metadata {
    Metadata {
        pallets: pallets.into_iter().map(convert_raw_pallet).collect(),
        extrinsic: convert_raw_extrinsic(extrinsic),
        registry,
    }
}

/// Returns true if the type resolves to a zero-size type (StructUnit or empty Tuple).
pub fn is_zero_size_type(ty: TypeId, registry: &scales::Registry) -> bool {
    match registry.resolve(ty) {
        Some(scales::TypeDef::StructUnit) => true,
        Some(scales::TypeDef::Tuple(t)) => t.is_empty(),
        _ => false,
    }
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

impl Metadata {
    pub fn from_bytes(bytes: &[u8]) -> crate::Result<Metadata> {
        from_bytes(bytes)
    }

    pub fn from_bytes_filtered(bytes: &[u8], pallet_filter: &[&str]) -> crate::Result<Metadata> {
        from_bytes_filtered(bytes, pallet_filter)
    }

    pub fn pallet_by_name(&self, name: &str) -> Option<&PalletMeta> {
        self.pallets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

// --- Storage key building ---

#[derive(Clone, Debug)]
pub enum KeyValue {
    Empty(TypeId),
    Value((TypeId, Vec<u8>, Vec<u8>, Hasher)),
}

pub struct StorageKey {
    pub pallet: Vec<u8>,
    pub call: Vec<u8>,
    pub args: Vec<KeyValue>,
    pub hashers: Vec<Hasher>,
    pub ty: TypeId,
}

impl StorageKey {
    pub fn new(
        ty: TypeId,
        pallet: Vec<u8>,
        call: Vec<u8>,
        args: Vec<KeyValue>,
        hashers: Vec<Hasher>,
    ) -> Self {
        Self {
            ty,
            pallet,
            call,
            args,
            hashers,
        }
    }

    pub fn key(&self) -> Vec<u8> {
        let args = self
            .args
            .iter()
            .map(|e| match e {
                KeyValue::Empty(_) => &[][..],
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
        registry: &scales::Registry,
        meta: &PalletMeta,
        item: &str,
        map_keys: &[T],
    ) -> crate::Result<Self> {
        let entry = meta
            .storage
            .as_ref()
            .and_then(|s| s.entries.iter().find(|e| e.name == item))
            .ok_or(crate::Error::StorageKeyNotFound)?;
        entry
            .ty
            .build_key(registry, &meta.name, &entry.name, map_keys)
    }
}

impl core::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "0x{}", hex::encode(self.key()))
    }
}

fn extract_tuple_type(key_id: TypeId, registry: &scales::Registry) -> Vec<TypeId> {
    match registry.resolve(key_id) {
        Some(scales::TypeDef::Tuple(types) | scales::TypeDef::StructTuple(types)) => types.to_vec(),
        _ => vec![key_id],
    }
}

impl StorageEntryType {
    fn build_key<T: AsRef<str>>(
        &self,
        registry: &scales::Registry,
        pallet: &str,
        item: &str,
        map_keys: &[T],
    ) -> crate::Result<StorageKey> {
        match self {
            Self::Plain(ty) => build_storage_key(
                registry,
                None,
                *ty,
                (pallet, item),
                &[] as &[&str],
                &[] as &[Hasher],
            ),
            Self::Map {
                hashers,
                key,
                value,
            } => build_storage_key(
                registry,
                Some(*key),
                *value,
                (pallet, item),
                map_keys,
                hashers,
            ),
        }
    }
}

fn build_storage_key<T: AsRef<str>>(
    registry: &scales::Registry,
    key_ty_id: Option<TypeId>,
    value_ty_id: TypeId,
    pallet_item: (&str, &str),
    map_keys: &[T],
    hashers: &[Hasher],
) -> crate::Result<StorageKey> {
    let type_call_ids = if let Some(key_ty_id) = key_ty_id {
        extract_tuple_type(key_ty_id, registry)
    } else {
        vec![]
    };

    if type_call_ids.len() == hashers.len() {
        let storage_key = StorageKey::new(
            value_ty_id,
            hash(&Hasher::Twox128, pallet_item.0),
            hash(&Hasher::Twox128, pallet_item.1),
            type_call_ids
                .into_iter()
                .enumerate()
                .map(|(i, type_id)| {
                    let Some(k) = map_keys.get(i) else {
                        return KeyValue::Empty(type_id);
                    };
                    let out = encode_key(k.as_ref(), registry, type_id);
                    let hashed = hash(&hashers[i], &out);
                    KeyValue::Value((type_id, hashed, out, hashers[i].clone()))
                })
                .collect(),
            hashers.to_vec(),
        );
        Ok(storage_key)
    } else if hashers.len() == 1 {
        let mut tuple_bytes = Vec::new();
        for (i, type_id) in type_call_ids.into_iter().enumerate() {
            let k = map_keys.get(i).ok_or(crate::Error::BadInput)?;
            tuple_bytes.extend(encode_key(k.as_ref(), registry, type_id));
        }
        let hasher = &hashers[0];
        let hashed_value = hash(hasher, &tuple_bytes);
        let key_ty = key_ty_id.ok_or(crate::Error::BadInput)?;
        Ok(StorageKey::new(
            value_ty_id,
            hash(&Hasher::Twox128, pallet_item.0),
            hash(&Hasher::Twox128, pallet_item.1),
            vec![KeyValue::Value((
                key_ty,
                hashed_value,
                tuple_bytes,
                hasher.clone(),
            ))],
            hashers.to_vec(),
        ))
    } else {
        Err(crate::Error::Encode(
            "Wrong number of hashers vs map_keys".into(),
        ))
    }
}
