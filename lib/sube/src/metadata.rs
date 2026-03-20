use crate::prelude::*;

use codec::Decode;
use scales::to_bytes_with_info;
use serde::Serialize;

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
/// The full `PortableRegistry` is dropped after compression into `scales::Registry`.
#[derive(Clone, Debug)]
pub struct Metadata {
    pub pallets: Vec<PalletMeta>,
    pub extrinsic: ExtrinsicMeta,
    pub registry: scales::Registry,
}

impl Serialize for Metadata {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> core::result::Result<S::Ok, S::Error> {
        self.registry.serialize(serializer)
    }
}

/// Decode metadata from its raw prefixed format.
/// Supports V14, V15, and V16 depending on enabled features.
/// The full PortableRegistry is compressed into a scales::Registry
/// and then dropped — only the compressed form is kept.
pub fn from_bytes(bytes: &mut &[u8]) -> core::result::Result<Metadata, codec::Error> {
    use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};
    use scale_info::form::PortableForm;

    type FmHasher = frame_metadata::v14::StorageHasher;

    fn convert_hasher(h: &FmHasher) -> Hasher {
        match h {
            FmHasher::Blake2_128 => Hasher::Blake2_128,
            FmHasher::Blake2_256 => Hasher::Blake2_256,
            FmHasher::Blake2_128Concat => Hasher::Blake2_128Concat,
            FmHasher::Twox128 => Hasher::Twox128,
            FmHasher::Twox256 => Hasher::Twox256,
            FmHasher::Twox64Concat => Hasher::Twox64Concat,
            FmHasher::Identity => Hasher::Identity,
        }
    }

    fn convert_entry_type(
        ty: &frame_metadata::v14::StorageEntryType<PortableForm>,
    ) -> StorageEntryType {
        match ty {
            frame_metadata::v14::StorageEntryType::Plain(t) => StorageEntryType::Plain(t.id),
            frame_metadata::v14::StorageEntryType::Map {
                hashers,
                key,
                value,
            } => StorageEntryType::Map {
                hashers: hashers.iter().map(convert_hasher).collect(),
                key: key.id,
                value: value.id,
            },
        }
    }

    // V14/V15/V16 PalletMetadata are distinct types with identical fields.
    macro_rules! convert_pallet {
        ($p:expr) => {{
            let p = $p;
            PalletMeta {
                name: p.name,
                index: p.index,
                calls_ty: p.calls.map(|c| c.ty.id),
                storage: p.storage.map(|s| StorageMeta {
                    prefix: s.prefix,
                    entries: s
                        .entries
                        .into_iter()
                        .map(|e| StorageEntryMeta {
                            name: e.name,
                            ty: convert_entry_type(&e.ty),
                        })
                        .collect(),
                }),
                constants: p
                    .constants
                    .into_iter()
                    .map(|c| ConstantMeta {
                        name: c.name,
                        ty: c.ty.id,
                        value: c.value,
                    })
                    .collect(),
            }
        }};
    }

    let meta: RuntimeMetadataPrefixed = Decode::decode(bytes)?;
    let (types, pallets, extrinsic) = match meta.1 {
        RuntimeMetadata::V14(m) => {
            let pallets = m.pallets.into_iter().map(|p| convert_pallet!(p)).collect();
            let extrinsic = ExtrinsicMeta {
                version: m.extrinsic.version,
                address_ty: None,
                signature_ty: None,
                extensions: m
                    .extrinsic
                    .signed_extensions
                    .into_iter()
                    .map(|e| SignedExtensionMeta {
                        identifier: e.identifier,
                        ty: e.ty.id,
                        additional_signed: e.additional_signed.id,
                    })
                    .collect(),
            };
            (m.types, pallets, extrinsic)
        }
        RuntimeMetadata::V15(m) => {
            let pallets = m.pallets.into_iter().map(|p| convert_pallet!(p)).collect();
            let extrinsic = ExtrinsicMeta {
                version: m.extrinsic.version,
                address_ty: Some(m.extrinsic.address_ty.id),
                signature_ty: Some(m.extrinsic.signature_ty.id),
                extensions: m
                    .extrinsic
                    .signed_extensions
                    .into_iter()
                    .map(|e| SignedExtensionMeta {
                        identifier: e.identifier,
                        ty: e.ty.id,
                        additional_signed: e.additional_signed.id,
                    })
                    .collect(),
            };
            (m.types, pallets, extrinsic)
        }
        RuntimeMetadata::V16(m) => {
            let pallets = m.pallets.into_iter().map(|p| convert_pallet!(p)).collect();
            // Pick highest supported version
            let version = m.extrinsic.versions.iter().copied().max().unwrap_or(4);
            // Get ordered extension indices for this version, or fall back to all
            let ext_indices: Vec<u32> = m
                .extrinsic
                .transaction_extensions_by_version
                .get(&version)
                .map(|idxs| idxs.iter().map(|c| c.0).collect())
                .unwrap_or_else(|| (0..m.extrinsic.transaction_extensions.len() as u32).collect());
            let extrinsic = ExtrinsicMeta {
                version,
                address_ty: Some(m.extrinsic.address_ty.id),
                signature_ty: Some(m.extrinsic.signature_ty.id),
                extensions: ext_indices
                    .into_iter()
                    .filter_map(|i| {
                        m.extrinsic.transaction_extensions.get(i as usize).map(|e| {
                            SignedExtensionMeta {
                                identifier: e.identifier.clone(),
                                ty: e.ty.id,
                                additional_signed: e.implicit.id,
                            }
                        })
                    })
                    .collect(),
            };
            (m.types, pallets, extrinsic)
        }
        _ => return Err(codec::Error::from("Metadata version not supported")),
    };

    let registry = scales::compress::compress(&types)
        .map_err(|_| codec::Error::from("Failed to compress registry"))?;

    Ok(Metadata {
        pallets,
        extrinsic,
        registry,
    })
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
    pub fn pallet_by_name(&self, name: &str) -> Option<&PalletMeta> {
        self.pallets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Debug)]
pub enum KeyValue {
    Empty(TypeId),
    // type id, hash, encoded_value, hasher
    Value((TypeId, Vec<u8>, Vec<u8>, Hasher)),
}

/// Represents a key of the blockchain storage in its raw form
#[derive(Clone, Debug)]
pub struct StorageKey {
    pub pallet: Vec<u8>,
    pub call: Vec<u8>,
    pub args: Vec<KeyValue>,
    pub ty: TypeId,
}

impl StorageKey {
    pub fn new(ty: TypeId, pallet: Vec<u8>, call: Vec<u8>, args: Vec<KeyValue>) -> Self {
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
        log::trace!(
            "map_keys={}",
            map_keys
                .iter()
                .map(|x| x.as_ref())
                .collect::<Vec<&str>>()
                .join(", ")
        );
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
        Some(scales::TypeDef::Tuple(types) | scales::TypeDef::StructTuple(types)) => types.clone(),
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
            } => {
                log::trace!("key={}, value={}, hasher={:?}", key, value, hashers);
                build_storage_key(
                    registry,
                    Some(*key),
                    *value,
                    (pallet, item),
                    map_keys,
                    hashers,
                )
            }
        }
    }
}

/// Decode metadata from a full `RuntimeMetadataPrefixed` SCALE blob.
/// Convenience wrapper that handles the `&mut &[u8]` slice.
impl Metadata {
    pub fn from_bytes(bytes: &[u8]) -> core::result::Result<Metadata, codec::Error> {
        from_bytes(&mut &bytes[..])
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
        log::trace!("resolving key type id={}", key_ty_id);
        extract_tuple_type(key_ty_id, registry)
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
                    let hasher = &hashers[i];

                    let Some(k) = k else {
                        return KeyValue::Empty(type_id);
                    };

                    let k = k.as_ref();
                    let out = encode_key(k, registry, type_id);

                    let hashed = hash(hasher, &out);
                    KeyValue::Value((type_id, hashed, out, hasher.clone()))
                })
                .collect(),
        );
        Ok(storage_key)
    } else if hashers.len() == 1 {
        log::trace!("treating tuple as argument for hasher");

        let mut tuple_bytes = Vec::new();
        for (i, type_id) in type_call_ids.into_iter().enumerate() {
            let k = map_keys.get(i).ok_or(crate::Error::BadInput)?;
            tuple_bytes.extend(encode_key(k.as_ref(), registry, type_id));
        }

        let hasher = &hashers[0];
        let hashed_value = hash(hasher, &tuple_bytes);
        let key_ty = key_ty_id.ok_or(crate::Error::BadInput)?;

        let storage_key = StorageKey::new(
            value_ty_id,
            hash(&Hasher::Twox128, pallet_item.0),
            hash(&Hasher::Twox128, pallet_item.1),
            vec![KeyValue::Value((
                key_ty,
                hashed_value,
                tuple_bytes,
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

#[cfg(test)]
mod tests {
    use super::*;

    // V15 metadata from the Kreivo parachain
    const KREIVO_METADATA: &[u8] = include_bytes!("../../../sdk/js/.papi/metadata/kreivo.scale");

    fn kreivo() -> Metadata {
        Metadata::from_bytes(KREIVO_METADATA).expect("kreivo metadata decodes")
    }

    #[test]
    fn decode_v15_metadata() {
        let meta = kreivo();
        assert!(
            meta.pallets.len() > 10,
            "expected many pallets, got {}",
            meta.pallets.len()
        );
    }

    #[test]
    fn pallet_by_name_case_insensitive() {
        let meta = kreivo();
        assert!(meta.pallet_by_name("system").is_some());
        assert!(meta.pallet_by_name("System").is_some());
        assert!(meta.pallet_by_name("SYSTEM").is_some());
        assert!(meta.pallet_by_name("Balances").is_some());
    }

    #[test]
    fn pallet_by_name_not_found() {
        let meta = kreivo();
        assert!(meta.pallet_by_name("NonExistentPallet").is_none());
    }

    #[test]
    fn system_pallet_has_expected_storage() {
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();
        let storage = system.storage.as_ref().expect("System has storage");
        let account = storage.entries.iter().find(|e| e.name == "Account");
        assert!(account.is_some(), "System should have Account storage");
        assert!(
            matches!(
                &account.unwrap().ty,
                StorageEntryType::Map { hashers, .. } if !hashers.is_empty()
            ),
            "Account should be a Map with non-empty hashers"
        );
    }

    #[test]
    fn system_pallet_has_constants() {
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();
        let version = system.constants.iter().find(|c| c.name == "Version");
        assert!(version.is_some(), "System should have Version constant");
        let version = version.unwrap();
        assert!(!version.value.is_empty(), "Version should have data");
    }

    #[test]
    fn balances_pallet_has_calls() {
        let meta = kreivo();
        let balances = meta.pallet_by_name("Balances").unwrap();
        assert!(
            balances.calls_ty.is_some(),
            "Balances should have a calls type"
        );
    }

    #[test]
    fn registry_resolves_types() {
        let meta = kreivo();
        // Type 0 should always exist in a substrate registry
        assert!(
            meta.registry.resolve(0).is_some(),
            "Registry should resolve type 0"
        );
    }

    #[test]
    fn decode_system_version_constant() {
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();
        let version = system
            .constants
            .iter()
            .find(|c| c.name == "Version")
            .unwrap();
        let value = scales::Value::new(&version.value, version.ty, &meta.registry);
        let json: serde_json::Value = value.try_into().expect("Version decodes to JSON");
        let obj = json.as_object().expect("Version is an object");
        assert!(obj.contains_key("spec_name"), "Version has spec_name");
        assert!(obj.contains_key("spec_version"), "Version has spec_version");
    }

    #[test]
    fn storage_key_for_plain_entry() {
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();
        let key = StorageKey::build_with_registry(&meta.registry, system, "Number", &[] as &[&str]);
        assert!(key.is_ok(), "Should build key for plain storage");
        let key = key.unwrap();
        assert!(!key.is_partial());
        assert!(!key.key().is_empty());
    }

    #[test]
    fn storage_key_for_map_entry() {
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();
        // Account is a Map<AccountId32, AccountInfo>
        let key = StorageKey::build_with_registry(
            &meta.registry,
            system,
            "Account",
            &["0x0000000000000000000000000000000000000000000000000000000000000000"],
        );
        assert!(
            key.is_ok(),
            "Should build key for map storage: {:?}",
            key.err()
        );
        let key = key.unwrap();
        assert!(!key.is_partial());
    }

    #[test]
    fn storage_key_partial_map() {
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();
        // No map key provided → partial key
        let key =
            StorageKey::build_with_registry(&meta.registry, system, "Account", &[] as &[&str]);
        assert!(key.is_ok());
        assert!(key.unwrap().is_partial());
    }

    #[test]
    fn storage_entry_not_found() {
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();
        let key =
            StorageKey::build_with_registry(&meta.registry, system, "NonExistent", &[] as &[&str]);
        assert!(key.is_err());
    }

    #[test]
    fn invalid_metadata_bytes() {
        let result = Metadata::from_bytes(&[0, 1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn extrinsic_meta_from_kreivo() {
        let meta = kreivo();
        let ext = &meta.extrinsic;
        assert_eq!(ext.extensions.len(), 9, "Kreivo should have 9 extensions");
        let names: Vec<&str> = ext
            .extensions
            .iter()
            .map(|e| e.identifier.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "PassAuthenticate",
                "CheckNonZeroSender",
                "CheckSpecVersion",
                "CheckTxVersion",
                "CheckGenesis",
                "CheckMortality",
                "CheckNonce",
                "CheckWeight",
                "ChargeAssetTxPayment",
            ]
        );
    }

    #[test]
    fn extrinsic_version_is_4() {
        let meta = kreivo();
        assert_eq!(meta.extrinsic.version, 4);
    }

    #[test]
    fn unit_type_extensions() {
        let meta = kreivo();
        let ext = &meta.extrinsic;
        // CheckNonZeroSender and CheckWeight should have StructUnit for ty
        for name in &["CheckNonZeroSender", "CheckWeight"] {
            let e = ext
                .extensions
                .iter()
                .find(|e| e.identifier == *name)
                .unwrap();
            assert!(
                matches!(
                    meta.registry.resolve(e.ty),
                    Some(scales::TypeDef::StructUnit)
                ),
                "{name} ty should be StructUnit, got {:?}",
                meta.registry.resolve(e.ty)
            );
        }
        // additional_signed for these resolves to () — either StructUnit or empty Tuple
        for name in &["CheckNonZeroSender", "CheckWeight"] {
            let e = ext
                .extensions
                .iter()
                .find(|e| e.identifier == *name)
                .unwrap();
            let is_zero_size = is_zero_size_type(e.additional_signed, &meta.registry);
            assert!(
                is_zero_size,
                "{name} additional_signed should be zero-size, got {:?}",
                meta.registry.resolve(e.additional_signed)
            );
        }
    }

    #[test]
    fn data_type_extensions() {
        let meta = kreivo();
        let ext = &meta.extrinsic;
        // CheckNonce ty should NOT be StructUnit (it holds the nonce)
        let check_nonce = ext
            .extensions
            .iter()
            .find(|e| e.identifier == "CheckNonce")
            .unwrap();
        assert!(
            !matches!(
                meta.registry.resolve(check_nonce.ty),
                Some(scales::TypeDef::StructUnit)
            ),
            "CheckNonce ty should not be StructUnit"
        );
        // CheckSpecVersion additional_signed should NOT be StructUnit (it's u32)
        let check_spec = ext
            .extensions
            .iter()
            .find(|e| e.identifier == "CheckSpecVersion")
            .unwrap();
        assert!(
            !matches!(
                meta.registry.resolve(check_spec.additional_signed),
                Some(scales::TypeDef::StructUnit)
            ),
            "CheckSpecVersion additional_signed should not be StructUnit"
        );
    }

    #[test]
    fn address_and_signature_types() {
        let meta = kreivo();
        assert!(
            meta.extrinsic.address_ty.is_some(),
            "V15 should expose address_ty"
        );
        assert!(
            meta.extrinsic.signature_ty.is_some(),
            "V15 should expose signature_ty"
        );
    }

    /// Helper: find a storage map entry keyed by a u32 in any pallet.
    fn find_u32_map(meta: &Metadata) -> Option<(&PalletMeta, &StorageEntryMeta)> {
        for pallet in &meta.pallets {
            let storage = match &pallet.storage {
                Some(s) => s,
                None => continue,
            };
            for entry in &storage.entries {
                if let StorageEntryType::Map { key, hashers, .. } = &entry.ty {
                    if hashers.len() == 1 {
                        if let Some(scales::TypeDef::U32) = meta.registry.resolve(*key) {
                            return Some((pallet, entry));
                        }
                    }
                }
            }
        }
        None
    }

    #[test]
    fn text_format_numeric_key() {
        // Text format lets you write "42" for a u32 key instead of hex-encoded SCALE bytes
        let meta = kreivo();
        let (pallet, entry) = find_u32_map(&meta).expect("should have a u32-keyed map");

        let key =
            StorageKey::build_with_registry(&meta.registry, pallet, &entry.name, &["42"]).unwrap();

        assert!(!key.is_partial());
        // Verify encode_key produced correct little-endian u32 bytes
        assert!(
            matches!(&key.args[0], KeyValue::Value((_, _, encoded, _)) if encoded == &42u32.to_le_bytes()),
            "text '42' should encode as LE u32"
        );
    }

    #[test]
    fn encode_key_text_vs_manual_scale() {
        // Demonstrate that text format and manual SCALE encoding produce the same key
        let meta = kreivo();
        let (pallet, entry) = find_u32_map(&meta).expect("should have a u32-keyed map");

        let text_key =
            StorageKey::build_with_registry(&meta.registry, pallet, &entry.name, &["100"]).unwrap();

        // Manually encode 100u32 as hex SCALE (little-endian)
        let hex_key =
            StorageKey::build_with_registry(&meta.registry, pallet, &entry.name, &["0x64000000"])
                .unwrap();

        assert_eq!(
            text_key.key(),
            hex_key.key(),
            "text '100' and hex '0x64000000' should produce identical storage keys"
        );
    }

    #[test]
    fn storage_entry_to_text_roundtrip() {
        // Decode a constant to text, showing the text format output
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();
        let version = system
            .constants
            .iter()
            .find(|c| c.name == "Version")
            .unwrap();
        let entry = crate::StorageEntry::new(version.value.clone(), version.ty);

        let text = entry.to_text(&meta.registry).expect("formats as text");
        assert!(
            text.contains("kreivo"),
            "Version text should contain the spec name"
        );

        // The text output can be fed back into from_text to re-encode
        let re_encoded = scales::from_text(&text, &meta.registry, version.ty)
            .expect("text output should round-trip through from_text");
        assert_eq!(
            re_encoded, version.value,
            "round-trip should reproduce original bytes"
        );
    }

    #[test]
    fn text_format_account_id_hex() {
        // AccountId32 wraps [u8; 32] — text format accepts 0x hex for byte arrays
        let meta = kreivo();
        let system = meta.pallet_by_name("System").unwrap();

        let addr = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";

        let key =
            StorageKey::build_with_registry(&meta.registry, system, "Account", &[addr]).unwrap();

        assert!(!key.is_partial());
        match &key.args[0] {
            KeyValue::Value((_, _, encoded, _)) => {
                assert_eq!(encoded.len(), 32, "AccountId32 should be 32 bytes");
                assert_eq!(encoded[0], 0xd4, "first byte should match");
            }
            other => assert!(false, "expected a Value, got {:?}", other),
        }

        // from_text now handles this directly (no hex fallback needed)
        let account = system
            .storage
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.name == "Account")
            .unwrap();
        let key_ty = match &account.ty {
            StorageEntryType::Map { key, .. } => *key,
            other => {
                assert!(false, "Account should be a Map, got {:?}", other);
                return;
            }
        };
        assert!(
            scales::from_text(addr, &meta.registry, key_ty).is_ok(),
            "from_text should handle 0x hex for byte arrays"
        );
    }
}
