//! Streaming metadata decode for memory-constrained targets.
//!
//! Two-pass approach using [`StreamCursor`] to avoid holding the full
//! metadata blob in memory:
//!
//! 1. **Scan pass** ([`scan_metadata`]): extract type reference graph + pallets (~35KB peak)
//! 2. **Decode pass** ([`decode_needed_types`]): decode only types in the needed set (~80KB peak)

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

use embedded_io_async::Read;

use super::metadata::{RawExtrinsic, RawPallet};
use super::stream_cursor::StreamCursor;
use crate::registry::TypeDefOwned;
use crate::Error;

/// Single-pass streaming metadata decode.
///
/// Decodes ALL types (builds the full registry) and filters pallets.
/// Simpler than two-pass, and the full Kreivo registry is only ~120KB
/// which fits when combined with ~50KB connection overhead.
/// Peak memory: ~170KB (types accumulate as decoded, then compacted into registry).
pub async fn decode_metadata_streaming<R: Read>(
    reader: R,
    pallet_filter: &[&str],
) -> Result<super::metadata::RawMetadata, Error> {
    use super::metadata::RawMetadata;

    let mut c = StreamCursor::new(reader);
    let version = read_header_async(&mut c).await?;

    let type_count = c.read_compact_u32().await?;
    let mut types = Vec::with_capacity(type_count as usize);
    for _ in 0..type_count {
        let id = c.read_compact_u32().await?;
        let td = decode_portable_type_async(&mut c).await?;
        // Fill gaps (type IDs may not be contiguous)
        while types.len() < id as usize {
            types.push(TypeDefOwned::StructUnit);
        }
        types.push(td);
    }
    super::metadata::postprocess_types(&mut types);

    let pallet_count = c.read_compact_u32().await?;
    let mut pallets = Vec::with_capacity(pallet_count as usize);
    for _ in 0..pallet_count {
        let p = decode_pallet_async(&mut c, version).await?;
        if p.name == "System" || pallet_filter.iter().any(|n| p.name == *n) {
            pallets.push(p);
        }
    }
    let extrinsic = decode_extrinsic_async(&mut c, version).await?;
    Ok(RawMetadata {
        types,
        pallets,
        extrinsic,
    })
}

/// Result of a metadata scan pass.
pub struct ScanResult {
    /// Filtered pallets (only those matching the filter + System).
    pub pallets: Vec<RawPallet>,
    /// Extrinsic metadata.
    pub extrinsic: RawExtrinsic,
    /// Flat buffer of all type references.
    ref_data: Vec<u32>,
    /// Per-type offset+length into `ref_data`.
    ref_index: Vec<(u32, u16)>,
    /// Total number of types in the registry.
    pub type_count: u32,
}

impl ScanResult {
    /// Get type references for a given type ID.
    fn refs_for(&self, id: u32) -> &[u32] {
        self.ref_index
            .get(id as usize)
            .map(|&(off, len)| &self.ref_data[off as usize..(off as usize + len as usize)])
            .unwrap_or(&[])
    }

    /// Drop the reference data to free memory before pass 2.
    pub fn drop_refs(&mut self) {
        self.ref_data = Vec::new();
        self.ref_index = Vec::new();
    }
}

/// Pass 1: Stream through metadata, extracting only type references and pallets.
///
/// Types are not decoded — only their referenced type IDs are extracted.
/// Uses a flat buffer (~8KB) instead of per-type Vecs (~14KB + allocation overhead).
/// Pallets are fully decoded and filtered. Peak memory: ~30KB.
pub async fn scan_metadata<R: Read>(
    reader: R,
    pallet_filter: &[&str],
) -> Result<ScanResult, Error> {
    let mut c = StreamCursor::new(reader);
    let version = read_header_async(&mut c).await?;

    // Scan types — extract only references into flat buffer
    let type_count = c.read_compact_u32().await?;
    // Preallocate: ~3 refs per type on average
    let mut ref_data = Vec::with_capacity((type_count as usize) * 3);
    let mut ref_index = Vec::with_capacity(type_count as usize);
    let mut tmp_refs = Vec::new();
    for _ in 0..type_count {
        let _id = c.read_compact_u32().await?;
        tmp_refs.clear();
        scan_type_refs(&mut c, &mut tmp_refs).await?;
        let offset = ref_data.len() as u32;
        ref_data.extend_from_slice(&tmp_refs);
        ref_index.push((offset, tmp_refs.len() as u16));
    }

    // Decode pallets
    let pallet_count = c.read_compact_u32().await?;
    let mut all_pallets = Vec::with_capacity(pallet_count as usize);
    for _ in 0..pallet_count {
        all_pallets.push(decode_pallet_async(&mut c, version).await?);
    }
    let extrinsic = decode_extrinsic_async(&mut c, version).await?;

    // Filter pallets
    let pallets: Vec<RawPallet> = all_pallets
        .into_iter()
        .filter(|p| p.name == "System" || pallet_filter.iter().any(|n| p.name == *n))
        .collect();

    Ok(ScanResult {
        pallets,
        extrinsic,
        ref_data,
        ref_index,
        type_count,
    })
}

/// Compute the transitive closure of type IDs needed by the given pallets.
pub fn resolve_needed_types(scan: &ScanResult) -> BTreeSet<u32> {
    let mut root_ids = super::metadata::collect_pallet_type_ids(&scan.pallets, &scan.extrinsic);
    let mut needed = BTreeSet::new();
    while let Some(id) = root_ids.pop() {
        if !needed.insert(id) {
            continue;
        }
        root_ids.extend_from_slice(scan.refs_for(id));
    }
    needed
}

/// Pass 2: Stream through metadata, decoding only types in the needed set.
///
/// Returns the decoded types (with remapped IDs) and the ID remap table.
pub async fn decode_needed_types<R: Read>(
    reader: R,
    needed: &BTreeSet<u32>,
    type_count: u32,
) -> Result<(Vec<TypeDefOwned>, Vec<Option<u32>>), Error> {
    let mut c = StreamCursor::new(reader);
    let _version = read_header_async(&mut c).await?;

    // Build ID remap
    let mut id_map: Vec<Option<u32>> = Vec::new();
    id_map.resize(type_count as usize, None);
    let mut new_id = 0u32;
    for &old_id in needed {
        if (old_id as usize) < id_map.len() {
            id_map[old_id as usize] = Some(new_id);
            new_id += 1;
        }
    }

    // Read types — decode needed, skip rest
    let count = c.read_compact_u32().await?;
    let mut types = Vec::with_capacity(needed.len());
    for _ in 0..count {
        let id = c.read_compact_u32().await?;
        if needed.contains(&id) {
            let mut td = decode_portable_type_async(&mut c).await?;
            super::metadata::remap_type_ids(&mut td, &id_map);
            types.push(td);
        } else {
            skip_portable_type_async(&mut c).await?;
        }
    }

    super::metadata::postprocess_types(&mut types);

    Ok((types, id_map))
}

// --- Async header ---

async fn read_header_async<R: Read>(c: &mut StreamCursor<R>) -> Result<u8, Error> {
    let magic = c.read_u32_le().await?;
    if magic != 0x6174656d {
        return Err(Error::BadInput("not metadata (bad magic)".into()));
    }
    c.read_byte().await
}

// --- Async type scanning (extract refs only) ---

/// Extract type IDs referenced by this type into `refs`, without full decode.
async fn scan_type_refs<R: Read>(
    c: &mut StreamCursor<R>,
    refs: &mut Vec<u32>,
) -> Result<(), Error> {
    // path
    c.skip_vec_string().await?;
    // type_params
    let param_count = c.read_compact_u32().await?;
    for _ in 0..param_count {
        c.skip_string().await?;
        if c.read_byte().await? != 0 {
            refs.push(c.read_compact_u32().await?);
        }
    }
    // type_def — scan for type IDs
    scan_type_def_refs(c, refs).await?;
    // docs
    c.skip_vec_string().await?;
    Ok(())
}

async fn scan_type_def_refs<R: Read>(
    c: &mut StreamCursor<R>,
    refs: &mut Vec<u32>,
) -> Result<(), Error> {
    match c.read_byte().await? {
        0 => {
            // Composite
            let count = c.read_compact_u32().await?;
            for _ in 0..count {
                if c.read_byte().await? != 0 {
                    c.skip_string().await?;
                }
                refs.push(c.read_compact_u32().await?);
                if c.read_byte().await? != 0 {
                    c.skip_string().await?;
                }
                c.skip_vec_string().await?;
            }
        }
        1 => {
            // Variant
            let count = c.read_compact_u32().await?;
            for _ in 0..count {
                c.skip_string().await?;
                let fc = c.read_compact_u32().await?;
                for _ in 0..fc {
                    if c.read_byte().await? != 0 {
                        c.skip_string().await?;
                    }
                    refs.push(c.read_compact_u32().await?);
                    if c.read_byte().await? != 0 {
                        c.skip_string().await?;
                    }
                    c.skip_vec_string().await?;
                }
                c.read_byte().await?; // index
                c.skip_vec_string().await?;
            }
        }
        2 => refs.push(c.read_compact_u32().await?),
        3 => {
            c.read_u32_le().await?;
            refs.push(c.read_compact_u32().await?);
        }
        4 => {
            let n = c.read_compact_u32().await?;
            for _ in 0..n {
                refs.push(c.read_compact_u32().await?);
            }
        }
        5 => {
            c.read_byte().await?;
        }
        6 => refs.push(c.read_compact_u32().await?),
        7 => {
            refs.push(c.read_compact_u32().await?);
            refs.push(c.read_compact_u32().await?);
        }
        _ => return Err(Error::BadInput("unknown TypeDef variant".into())),
    }
    Ok(())
}

// --- Async type skip ---

async fn skip_portable_type_async<R: Read>(c: &mut StreamCursor<R>) -> Result<(), Error> {
    c.skip_vec_string().await?; // path
    let param_count = c.read_compact_u32().await?;
    for _ in 0..param_count {
        c.skip_string().await?;
        c.skip_option_compact_u32().await?;
    }
    skip_type_def_async(c).await?;
    c.skip_vec_string().await?; // docs
    Ok(())
}

async fn skip_type_def_async<R: Read>(c: &mut StreamCursor<R>) -> Result<(), Error> {
    match c.read_byte().await? {
        0 => {
            let count = c.read_compact_u32().await?;
            for _ in 0..count {
                if c.read_byte().await? != 0 {
                    c.skip_string().await?;
                }
                c.read_compact_u32().await?;
                if c.read_byte().await? != 0 {
                    c.skip_string().await?;
                }
                c.skip_vec_string().await?;
            }
        }
        1 => {
            let count = c.read_compact_u32().await?;
            for _ in 0..count {
                c.skip_string().await?;
                let fc = c.read_compact_u32().await?;
                for _ in 0..fc {
                    if c.read_byte().await? != 0 {
                        c.skip_string().await?;
                    }
                    c.read_compact_u32().await?;
                    if c.read_byte().await? != 0 {
                        c.skip_string().await?;
                    }
                    c.skip_vec_string().await?;
                }
                c.read_byte().await?;
                c.skip_vec_string().await?;
            }
        }
        2 => {
            c.read_compact_u32().await?;
        }
        3 => {
            c.read_u32_le().await?;
            c.read_compact_u32().await?;
        }
        4 => {
            let n = c.read_compact_u32().await?;
            for _ in 0..n {
                c.read_compact_u32().await?;
            }
        }
        5 => {
            c.read_byte().await?;
        }
        6 => {
            c.read_compact_u32().await?;
        }
        7 => {
            c.read_compact_u32().await?;
            c.read_compact_u32().await?;
        }
        _ => return Err(Error::BadInput("unknown TypeDef variant".into())),
    }
    Ok(())
}

// --- Async type decode (full) ---

async fn decode_portable_type_async<R: Read>(
    c: &mut StreamCursor<R>,
) -> Result<TypeDefOwned, Error> {
    #[allow(unused_imports)]
    use crate::registry::*;

    // path — check for BTreeMap
    let path_count = c.read_compact_u32().await?;
    let mut is_btreemap = false;
    for i in 0..path_count {
        let s = c.read_string().await?;
        if i == path_count - 1 && s == "BTreeMap" {
            is_btreemap = true;
        }
    }
    // type_params
    let param_count = c.read_compact_u32().await?;
    for _ in 0..param_count {
        c.skip_string().await?;
        c.skip_option_compact_u32().await?;
    }
    // type_def
    let td = decode_type_def_async(c, is_btreemap).await?;
    // docs
    c.skip_vec_string().await?;
    Ok(td)
}

async fn decode_type_def_async<R: Read>(
    c: &mut StreamCursor<R>,
    is_btreemap: bool,
) -> Result<TypeDefOwned, Error> {
    use crate::registry::*;

    match c.read_byte().await? {
        0 => decode_composite_async(c, is_btreemap).await,
        1 => decode_variant_async(c).await,
        2 => Ok(TypeDefOwned::Sequence(c.read_compact_u32().await?)),
        3 => {
            let len = c.read_u32_le().await?;
            Ok(TypeDefOwned::Array(c.read_compact_u32().await?, len))
        }
        4 => {
            let count = c.read_compact_u32().await?;
            let mut ids = Vec::with_capacity(count as usize);
            for _ in 0..count {
                ids.push(c.read_compact_u32().await?);
            }
            Ok(TypeDefOwned::Tuple(ids))
        }
        5 => Ok(match c.read_byte().await? {
            0 => TypeDefOwned::Bool,
            1 => TypeDefOwned::Char,
            2 => TypeDefOwned::Str,
            3 => TypeDefOwned::U8,
            4 => TypeDefOwned::U16,
            5 => TypeDefOwned::U32,
            6 => TypeDefOwned::U64,
            7 => TypeDefOwned::U128,
            8 => TypeDefOwned::U128,
            9 => TypeDefOwned::I8,
            10 => TypeDefOwned::I16,
            11 => TypeDefOwned::I32,
            12 => TypeDefOwned::I64,
            13 => TypeDefOwned::I128,
            _ => return Err(Error::BadInput("unknown primitive".into())),
        }),
        6 => Ok(TypeDefOwned::Compact(c.read_compact_u32().await?)),
        7 => Ok(TypeDefOwned::BitSequence(
            c.read_compact_u32().await?,
            c.read_compact_u32().await?,
        )),
        _ => Err(Error::BadInput("unknown TypeDef variant".into())),
    }
}

async fn decode_composite_async<R: Read>(
    c: &mut StreamCursor<R>,
    is_btreemap: bool,
) -> Result<TypeDefOwned, Error> {
    use crate::registry::*;

    let count = c.read_compact_u32().await?;
    let mut named = Vec::new();
    let mut unnamed = Vec::new();
    let mut has_names = true;
    for _ in 0..count {
        let name = if c.read_byte().await? != 0 {
            Some(c.read_string().await?)
        } else {
            has_names = false;
            None
        };
        let ty = c.read_compact_u32().await?;
        if c.read_byte().await? != 0 {
            c.skip_string().await?;
        }
        c.skip_vec_string().await?;
        if let Some(name) = name {
            named.push(FieldOwned { name, ty });
        }
        unnamed.push(ty);
    }
    Ok(if count == 0 {
        TypeDefOwned::StructUnit
    } else if is_btreemap && count == 1 {
        TypeDefOwned::Map(unnamed[0], unnamed[0])
    } else if !has_names && count == 1 {
        TypeDefOwned::StructNewType(unnamed[0])
    } else if !has_names {
        TypeDefOwned::StructTuple(unnamed)
    } else {
        TypeDefOwned::Struct(named)
    })
}

async fn decode_variant_async<R: Read>(c: &mut StreamCursor<R>) -> Result<TypeDefOwned, Error> {
    use crate::registry::*;

    let count = c.read_compact_u32().await?;
    let mut variants = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = c.read_string().await?;
        let field_count = c.read_compact_u32().await?;
        let mut named = Vec::new();
        let mut unnamed = Vec::new();
        let mut has_names = true;
        for _ in 0..field_count {
            let fname = if c.read_byte().await? != 0 {
                Some(c.read_string().await?)
            } else {
                has_names = false;
                None
            };
            let ty = c.read_compact_u32().await?;
            if c.read_byte().await? != 0 {
                c.skip_string().await?;
            }
            c.skip_vec_string().await?;
            if let Some(fname) = fname {
                named.push(FieldOwned { name: fname, ty });
            }
            unnamed.push(ty);
        }
        let index = c.read_byte().await?;
        c.skip_vec_string().await?;

        let fields = if field_count == 0 {
            FieldsOwned::Unit
        } else if !has_names && field_count == 1 {
            FieldsOwned::NewType(unnamed[0])
        } else if !has_names {
            FieldsOwned::Tuple(unnamed)
        } else {
            FieldsOwned::Struct(named)
        };

        variants.push(VariantOwned {
            index,
            name,
            fields,
        });
    }
    Ok(TypeDefOwned::Variant(VariantDefOwned {
        name: String::new(),
        variants,
    }))
}

// --- Async pallet/extrinsic decode ---

use super::metadata::{RawConstant, RawExtension, RawStorage, RawStorageEntry, RawStorageType};

async fn decode_pallet_async<R: Read>(
    c: &mut StreamCursor<R>,
    version: u8,
) -> Result<RawPallet, Error> {
    let name = c.read_string().await?;

    // storage: Option<StorageMetadata>
    let storage = if c.read_byte().await? != 0 {
        let prefix = c.read_string().await?;
        let entry_count = c.read_compact_u32().await?;
        let mut entries = Vec::with_capacity(entry_count as usize);
        for _ in 0..entry_count {
            entries.push(decode_storage_entry_async(c).await?);
        }
        Some(RawStorage { prefix, entries })
    } else {
        None
    };

    // calls: Option<{ ty }>
    let calls_ty = if c.read_byte().await? != 0 {
        Some(c.read_compact_u32().await?)
    } else {
        None
    };
    // event: Option<{ ty }>
    if c.read_byte().await? != 0 {
        c.read_compact_u32().await?;
    }

    // constants
    let const_count = c.read_compact_u32().await?;
    let mut constants = Vec::with_capacity(const_count as usize);
    for _ in 0..const_count {
        let cname = c.read_string().await?;
        let ty = c.read_compact_u32().await?;
        let value_len = c.read_compact_u32().await? as usize;
        let value = c.read_bytes(value_len).await?;
        c.skip_vec_string().await?; // docs
        constants.push(RawConstant {
            name: cname,
            ty,
            value,
        });
    }

    // error: Option<{ ty }>
    if c.read_byte().await? != 0 {
        c.read_compact_u32().await?;
    }
    let index = c.read_byte().await?;
    if version >= 15 {
        c.skip_vec_string().await?;
    }

    Ok(RawPallet {
        name,
        index,
        calls_ty,
        storage,
        constants,
    })
}

async fn decode_storage_entry_async<R: Read>(
    c: &mut StreamCursor<R>,
) -> Result<RawStorageEntry, Error> {
    let name = c.read_string().await?;
    let modifier = c.read_byte().await?;
    let ty = match c.read_byte().await? {
        0 => RawStorageType::Plain(c.read_compact_u32().await?),
        1 => {
            let hc = c.read_compact_u32().await?;
            let mut hashers = Vec::with_capacity(hc as usize);
            for _ in 0..hc {
                hashers.push(c.read_byte().await?);
            }
            let key = c.read_compact_u32().await?;
            let value = c.read_compact_u32().await?;
            RawStorageType::Map {
                hashers,
                key,
                value,
            }
        }
        _ => return Err(Error::BadInput("unknown storage type".into())),
    };
    let default_len = c.read_compact_u32().await? as usize;
    let default = c.read_bytes(default_len).await?;
    c.skip_vec_string().await?; // docs
    Ok(RawStorageEntry {
        name,
        modifier,
        ty,
        default,
    })
}

async fn decode_extrinsic_async<R: Read>(
    c: &mut StreamCursor<R>,
    version: u8,
) -> Result<RawExtrinsic, Error> {
    let ext_version = c.read_byte().await?;
    let address_ty = if version >= 15 {
        Some(c.read_compact_u32().await?)
    } else {
        None
    };
    // call_ty (V15) — skip
    if version >= 15 {
        c.read_compact_u32().await?;
    }
    let signature_ty = if version >= 15 {
        Some(c.read_compact_u32().await?)
    } else {
        None
    };
    // extra_ty (V15) — skip
    if version >= 15 {
        c.read_compact_u32().await?;
    }
    let ext_count = c.read_compact_u32().await?;
    let mut extensions = Vec::with_capacity(ext_count as usize);
    for _ in 0..ext_count {
        let identifier = c.read_string().await?;
        let ty = c.read_compact_u32().await?;
        let additional_signed = c.read_compact_u32().await?;
        extensions.push(RawExtension {
            identifier,
            ty,
            additional_signed,
        });
    }
    Ok(RawExtrinsic {
        version: ext_version,
        address_ty,
        signature_ty,
        extensions,
    })
}
