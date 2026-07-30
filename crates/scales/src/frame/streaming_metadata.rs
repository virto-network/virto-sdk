//! Streaming metadata decode for memory-constrained targets.
//!
//! Two-pass approach using [`StreamCursor`] — never holds the full blob:
//!
//! 1. **Pass 1** ([`scan_pallets_streaming`]): skip all types, decode + filter pallets (~20KB peak)
//! 2. **Pass 2** ([`decode_filtered_types`]): decode only types needed by kept pallets,
//!    resolving transitive dependencies in a single forward scan (~60KB peak for 1 pallet)

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

use embedded_io_async::Read;

use super::metadata::{RawExtrinsic, RawPallet};
use super::stream_cursor::StreamCursor;
use crate::registry::*;
use crate::Error;

/// Result of pass 1 — pallets + extrinsic + type count.
pub struct PalletScan {
    pub pallets: Vec<RawPallet>,
    pub extrinsic: RawExtrinsic,
    pub type_count: u32,
}

/// Pass 1: skip all types, decode + filter pallets.
///
/// Peak memory: ~20KB (just the kept pallets + extrinsic).
pub async fn scan_pallets_streaming<R: Read>(
    reader: R,
    pallet_filter: &[&str],
) -> Result<PalletScan, Error> {
    let mut c = StreamCursor::new(reader);
    let version = read_header_async(&mut c).await?;

    // Skip all types — zero allocation
    let type_count = c.read_compact_u32().await?;
    if type_count > 10_000 {
        return Err(Error::BadInput(alloc::format!(
            "type_count too large: {type_count} (metadata corrupt?)"
        )));
    }
    for _ in 0..type_count {
        c.read_compact_u32().await?; // id
        skip_portable_type_async(&mut c).await?;
    }

    let pallet_count = c.read_compact_u32().await?;
    if pallet_count > 500 {
        return Err(Error::BadInput(alloc::format!(
            "pallet_count too large: {pallet_count} (type skipping off?)"
        )));
    }
    let mut pallets = Vec::with_capacity(pallet_count as usize);
    for _ in 0..pallet_count {
        let p = decode_pallet_async(&mut c, version).await?;
        if p.name == "System" || pallet_filter.iter().any(|n| p.name == *n) {
            pallets.push(p);
        }
    }
    let extrinsic = decode_extrinsic_async(&mut c, version).await?;

    Ok(PalletScan {
        pallets,
        extrinsic,
        type_count,
    })
}

/// Pass 2: decode needed types, compact each into Registry immediately.
///
/// Decode needed types into a pre-allocated Registry.
///
/// The `registry` should be allocated BEFORE connecting (when heap is
/// unfragmented). Pass it in along with the reader from the connection.
/// Returns the ID remap table.
pub async fn decode_filtered_to_registry<R: Read>(
    reader: R,
    needed_seed: &BTreeSet<u32>,
    type_count: u32,
    registry: &mut crate::Registry,
) -> Result<alloc::collections::BTreeMap<u32, u32>, Error> {
    let mut c = StreamCursor::new(reader);
    let _version = read_header_async(&mut c).await?;

    let mut needed = needed_seed.clone();
    let actual_type_count = c.read_compact_u32().await?;
    let count = actual_type_count.min(type_count);

    // Use BTreeMap instead of Vec<Option<u32>> to save ~3KB
    // (246 entries in BTreeMap vs 740 Option<u32> slots)
    let mut id_map = alloc::collections::BTreeMap::new();
    let mut new_id = 0u32;

    for _ in 0..count {
        let id = c.read_compact_u32().await?;
        if needed.contains(&id) {
            let td = decode_portable_type_async(&mut c).await?;
            let mut refs = Vec::new();
            collect_type_refs(&td, &mut refs);
            for r in refs {
                needed.insert(r);
            }
            registry.push(td);
            id_map.insert(id, new_id);
            new_id += 1;
        } else {
            skip_portable_type_async(&mut c).await?;
        }
    }

    // TODO: the types in the registry still have old IDs in their fields.
    // Registry doesn't support in-place ID remapping after construction.
    // For now, the remap is applied to pallets/extrinsic by the caller.
    // Type-internal references (e.g. struct field types) will have old IDs
    // which resolve to wrong registry slots. This needs a registry.remap() method.

    Ok(id_map)
}

/// Pass 2 (alternative): decode only types needed by the filtered pallets.
///
/// Resolves transitive dependencies in a single forward scan:
/// when a needed type is decoded, its referenced type IDs are added
/// to the needed set. Types ahead in ID order will be decoded when reached.
///
/// Peak memory: ~60KB for 1-2 pallets (decoded types + ID remap table).
pub async fn decode_filtered_types<R: Read>(
    reader: R,
    scan: &PalletScan,
) -> Result<(Vec<TypeDefOwned>, Vec<Option<u32>>), Error> {
    let mut c = StreamCursor::new(reader);
    let _version = read_header_async(&mut c).await?;

    // Seed needed set from pallet + extrinsic type refs
    let root_ids = super::metadata::collect_pallet_type_ids(&scan.pallets, &scan.extrinsic);
    let mut needed: BTreeSet<u32> = root_ids.into_iter().collect();

    let type_count = c.read_compact_u32().await?;

    // Sparse storage: only needed types get allocated
    let mut decoded: Vec<(u32, TypeDefOwned)> = Vec::new();

    for _ in 0..type_count {
        let id = c.read_compact_u32().await?;
        if needed.contains(&id) {
            let td = decode_portable_type_async(&mut c).await?;
            // Add transitive deps
            let mut refs = Vec::new();
            collect_type_refs(&td, &mut refs);
            for r in refs {
                needed.insert(r);
            }
            decoded.push((id, td));
        } else {
            skip_portable_type_async(&mut c).await?;
        }
    }

    // Build ID remap: old → new (contiguous)
    let mut id_map: Vec<Option<u32>> = Vec::new();
    id_map.resize(type_count as usize, None);
    for (i, &(old_id, _)) in decoded.iter().enumerate() {
        if (old_id as usize) < id_map.len() {
            id_map[old_id as usize] = Some(i as u32);
        }
    }

    // Remap + collect
    let mut types: Vec<TypeDefOwned> = decoded
        .into_iter()
        .map(|(_, mut td)| {
            super::metadata::remap_type_ids(&mut td, &id_map);
            td
        })
        .collect();

    super::metadata::postprocess_types(&mut types);

    Ok((types, id_map))
}

// --- Async header ---

async fn read_header_async<R: Read>(c: &mut StreamCursor<R>) -> Result<u8, Error> {
    let magic = c.read_u32_le().await?;
    if magic != 0x6174656d {
        return Err(Error::BadInput(alloc::format!(
            "not metadata (magic={:#010x}, expected 0x6174656d)",
            magic
        )));
    }
    let version = c.read_byte().await?;
    if version != 14 && version != 15 {
        return Err(Error::BadInput(alloc::format!(
            "unsupported metadata version: {version}"
        )));
    }
    Ok(version)
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
    let path_count = c.read_compact_u32().await?;
    let mut is_btreemap = false;
    for i in 0..path_count {
        let s = c.read_string().await?;
        if i == path_count - 1 && s == "BTreeMap" {
            is_btreemap = true;
        }
    }
    let param_count = c.read_compact_u32().await?;
    for _ in 0..param_count {
        c.skip_string().await?;
        c.skip_option_compact_u32().await?;
    }
    let td = decode_type_def_async(c, is_btreemap).await?;
    c.skip_vec_string().await?;
    Ok(td)
}

async fn decode_type_def_async<R: Read>(
    c: &mut StreamCursor<R>,
    is_btreemap: bool,
) -> Result<TypeDefOwned, Error> {
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

    let calls_ty = if c.read_byte().await? != 0 {
        Some(c.read_compact_u32().await?)
    } else {
        None
    };
    if c.read_byte().await? != 0 {
        c.read_compact_u32().await?;
    } // event

    let const_count = c.read_compact_u32().await?;
    let mut constants = Vec::with_capacity(const_count as usize);
    for _ in 0..const_count {
        let cname = c.read_string().await?;
        let ty = c.read_compact_u32().await?;
        let value_len = c.read_compact_u32().await? as usize;
        let value = c.read_bytes(value_len).await?;
        c.skip_vec_string().await?;
        constants.push(RawConstant {
            name: cname,
            ty,
            value,
        });
    }

    if c.read_byte().await? != 0 {
        c.read_compact_u32().await?;
    } // error
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
    c.skip_vec_string().await?;
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
    if version == 14 {
        let _ty = c.read_compact_u32().await?;
        let ext_version = c.read_byte().await?;
        let count = c.read_compact_u32().await?;
        let mut extensions = Vec::with_capacity(count as usize);
        for _ in 0..count {
            extensions.push(RawExtension {
                identifier: c.read_string().await?,
                ty: c.read_compact_u32().await?,
                additional_signed: c.read_compact_u32().await?,
            });
        }
        Ok(RawExtrinsic {
            version: ext_version,
            address_ty: None,
            signature_ty: None,
            extensions,
        })
    } else {
        let ext_version = c.read_byte().await?;
        let address_ty = c.read_compact_u32().await?;
        let _call_ty = c.read_compact_u32().await?;
        let signature_ty = c.read_compact_u32().await?;
        let _extra_ty = c.read_compact_u32().await?;
        let count = c.read_compact_u32().await?;
        let mut extensions = Vec::with_capacity(count as usize);
        for _ in 0..count {
            extensions.push(RawExtension {
                identifier: c.read_string().await?,
                ty: c.read_compact_u32().await?,
                additional_signed: c.read_compact_u32().await?,
            });
        }
        Ok(RawExtrinsic {
            version: ext_version,
            address_ty: Some(address_ty),
            signature_ty: Some(signature_ty),
            extensions,
        })
    }
}

/// Collect type IDs referenced by a decoded type.
fn collect_type_refs(td: &TypeDefOwned, out: &mut Vec<u32>) {
    match td {
        TypeDefOwned::Sequence(id)
        | TypeDefOwned::StructNewType(id)
        | TypeDefOwned::Compact(id) => out.push(*id),
        TypeDefOwned::Map(k, v) | TypeDefOwned::BitSequence(k, v) => {
            out.push(*k);
            out.push(*v);
        }
        TypeDefOwned::Array(id, _) => out.push(*id),
        TypeDefOwned::Tuple(ids) | TypeDefOwned::StructTuple(ids) => out.extend(ids),
        TypeDefOwned::Struct(fields) => {
            for f in fields {
                out.push(f.ty);
            }
        }
        TypeDefOwned::Variant(vdef) => {
            for v in &vdef.variants {
                match &v.fields {
                    FieldsOwned::NewType(id) => out.push(*id),
                    FieldsOwned::Tuple(ids) => out.extend(ids),
                    FieldsOwned::Struct(fields) => {
                        for f in fields {
                            out.push(f.ty);
                        }
                    }
                    FieldsOwned::Unit => {}
                }
            }
        }
        _ => {}
    }
}
