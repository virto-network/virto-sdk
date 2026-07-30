//! Lean metadata decoder — parses Substrate V14/V15 metadata SCALE bytes
//! directly into a compressed [`Registry`] without `frame-metadata` or `scale-info`.
//!
//! This avoids the ~700KB peak allocation from `frame_metadata::Decode`
//! and enables metadata parsing on memory-constrained targets (ESP32).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::cursor::Cursor;
use crate::registry::*;
use crate::Error;

// --- Public types ---

pub struct RawPallet {
    pub name: String,
    pub index: u8,
    pub calls_ty: Option<u32>,
    pub storage: Option<RawStorage>,
    pub constants: Vec<RawConstant>,
}

pub struct RawStorage {
    pub prefix: String,
    pub entries: Vec<RawStorageEntry>,
}

pub struct RawStorageEntry {
    pub name: String,
    pub modifier: u8,
    pub ty: RawStorageType,
    pub default: Vec<u8>,
}

pub enum RawStorageType {
    Plain(u32),
    Map {
        hashers: Vec<u8>,
        key: u32,
        value: u32,
    },
}

pub struct RawConstant {
    pub name: String,
    pub ty: u32,
    pub value: Vec<u8>,
}

pub struct RawExtrinsic {
    pub version: u8,
    pub address_ty: Option<u32>,
    pub signature_ty: Option<u32>,
    pub extensions: Vec<RawExtension>,
}

pub struct RawExtension {
    pub identifier: String,
    pub ty: u32,
    pub additional_signed: u32,
}

pub struct RawMetadata {
    pub types: Vec<TypeDefOwned>,
    pub pallets: Vec<RawPallet>,
    pub extrinsic: RawExtrinsic,
}

// --- Lightweight pallet scan (for two-request metadata fetch) ---

/// Pallet summary from a lightweight metadata scan.
pub struct PalletSummary {
    pub name: String,
    pub index: u8,
}

/// Scan metadata to extract pallet names without decoding types or constant values.
///
/// This is the cheapest possible metadata parse — only allocates pallet names.
/// Use with a second call to [`decode_metadata_filtered`] to decode only needed types.
pub fn scan_pallets(data: &[u8]) -> Result<Vec<PalletSummary>, Error> {
    let mut c = Cursor::new(data);
    let version = read_header(&mut c)?;

    // Skip all types
    let type_count = c.read_compact_u32()?;
    for _ in 0..type_count {
        c.read_compact_u32()?; // id
        skip_portable_type(&mut c)?;
    }

    // Decode pallets (lightweight — skip storage, constants, etc.)
    let pallet_count = c.read_compact_u32()?;
    let mut pallets = Vec::with_capacity(pallet_count as usize);
    for _ in 0..pallet_count {
        pallets.push(scan_single_pallet(&mut c, version)?);
    }
    Ok(pallets)
}

/// Scan a single pallet — extract name and index, skip everything else.
fn scan_single_pallet(c: &mut Cursor, version: u8) -> Result<PalletSummary, Error> {
    let name = c.read_string()?;

    // storage: Option<StorageMetadata>
    if c.read_byte()? != 0 {
        c.skip_string()?; // prefix
        let entry_count = c.read_compact_u32()?;
        for _ in 0..entry_count {
            skip_storage_entry(c)?;
        }
    }

    // calls: Option<{ ty }>
    if c.read_byte()? != 0 {
        c.read_compact_u32()?;
    }
    // event: Option<{ ty }>
    if c.read_byte()? != 0 {
        c.read_compact_u32()?;
    }

    // constants: Vec<ConstantMetadata>
    let const_count = c.read_compact_u32()?;
    for _ in 0..const_count {
        c.skip_string()?; // name
        c.read_compact_u32()?; // ty
        let value_len = c.read_compact_u32()? as usize;
        c.read_bytes(value_len)?; // value
        c.skip_vec_string()?; // docs
    }

    // error: Option<{ ty }>
    if c.read_byte()? != 0 {
        c.read_compact_u32()?;
    }
    let index = c.read_byte()?;
    if version >= 15 {
        c.skip_vec_string()?;
    }

    Ok(PalletSummary { name, index })
}

fn skip_storage_entry(c: &mut Cursor) -> Result<(), Error> {
    c.skip_string()?; // name
    c.read_byte()?; // modifier
    match c.read_byte()? {
        0 => {
            c.read_compact_u32()?;
        } // Plain
        1 => {
            // Map
            let hc = c.read_compact_u32()?;
            for _ in 0..hc {
                c.read_byte()?;
            }
            c.read_compact_u32()?; // key
            c.read_compact_u32()?; // value
        }
        _ => return Err(Error::BadInput("unknown storage entry type".into())),
    }
    let default_len = c.read_compact_u32()? as usize;
    c.read_bytes(default_len)?;
    c.skip_vec_string()?; // docs
    Ok(())
}

// --- Two-pass filtered decode ---

/// Decode metadata keeping only selected pallets and their referenced types.
///
/// Two-pass approach with minimal peak memory:
/// 1. Skip registry types (no allocation), decode pallets
/// 2. Walk type references, decode only needed types
///
/// Peak memory is proportional to selected pallets' types, not total metadata.
pub fn decode_metadata_filtered(data: &[u8], pallet_filter: &[&str]) -> Result<RawMetadata, Error> {
    let mut c = Cursor::new(data);
    let version = read_header(&mut c)?;

    // Pass 1: record type byte offsets without decoding, then decode pallets
    let type_count = c.read_compact_u32()?;
    let mut type_offsets: Vec<(u32, usize)> = Vec::with_capacity(type_count as usize);
    for _ in 0..type_count {
        let start = c.pos;
        let id = c.read_compact_u32()?;
        skip_portable_type(&mut c)?;
        type_offsets.push((id, start));
    }

    let pallet_count = c.read_compact_u32()?;
    let mut all_pallets = Vec::with_capacity(pallet_count as usize);
    for _ in 0..pallet_count {
        all_pallets.push(decode_pallet(&mut c, version)?);
    }
    let extrinsic = decode_extrinsic(&mut c, version)?;

    // Filter pallets
    let keep: Vec<&str> = {
        let mut v: Vec<&str> = pallet_filter.to_vec();
        if !v.contains(&"System") {
            v.push("System");
        }
        v
    };
    let pallets: Vec<RawPallet> = all_pallets
        .into_iter()
        .filter(|p| keep.iter().any(|n| p.name == *n))
        .collect();

    // Collect root type IDs
    let mut root_ids = collect_pallet_type_ids(&pallets, &extrinsic);

    // Walk type graph
    use alloc::collections::BTreeSet;
    let mut needed = BTreeSet::new();
    while let Some(id) = root_ids.pop() {
        if !needed.insert(id) {
            continue;
        }
        if let Some((_, offset)) = type_offsets.get(id as usize) {
            let mut tc = Cursor::new(data);
            tc.pos = *offset;
            let (_, _, td) = decode_portable_type(&mut tc)?;
            collect_type_refs(&td, &mut root_ids);
        }
    }

    // Build ID remap: old → new (contiguous)
    let mut id_map: Vec<Option<u32>> = vec![None; type_count as usize];
    let mut new_id = 0u32;
    for &old_id in &needed {
        if (old_id as usize) < id_map.len() {
            id_map[old_id as usize] = Some(new_id);
            new_id += 1;
        }
    }

    // Pass 2: decode only needed types
    let mut types = Vec::with_capacity(needed.len());
    for &old_id in &needed {
        if let Some((_, offset)) = type_offsets.get(old_id as usize) {
            let mut tc = Cursor::new(data);
            tc.pos = *offset;
            let (_, _, mut td) = decode_portable_type(&mut tc)?;
            remap_type_ids(&mut td, &id_map);
            types.push(td);
        }
    }

    postprocess_types(&mut types);

    let remap = |id: u32| id_map.get(id as usize).copied().flatten().unwrap_or(id);
    let pallets = remap_pallet_ids(pallets, &remap);
    let extrinsic = remap_extrinsic_ids(extrinsic, &remap);

    Ok(RawMetadata {
        types,
        pallets,
        extrinsic,
    })
}

/// Decode all metadata without filtering (higher memory, simpler).
pub fn decode_metadata(data: &[u8]) -> Result<RawMetadata, Error> {
    let mut c = Cursor::new(data);
    let version = read_header(&mut c)?;

    let type_count = c.read_compact_u32()?;
    let mut types = Vec::with_capacity(type_count as usize);
    for _ in 0..type_count {
        let (id, _, td) = decode_portable_type(&mut c)?;
        while types.len() < id as usize {
            types.push(TypeDefOwned::StructUnit);
        }
        types.push(td);
    }

    postprocess_types(&mut types);

    let pallet_count = c.read_compact_u32()?;
    let mut pallets = Vec::with_capacity(pallet_count as usize);
    for _ in 0..pallet_count {
        pallets.push(decode_pallet(&mut c, version)?);
    }
    let extrinsic = decode_extrinsic(&mut c, version)?;

    Ok(RawMetadata {
        types,
        pallets,
        extrinsic,
    })
}

// --- Internal helpers ---

fn read_header(c: &mut Cursor) -> Result<u8, Error> {
    let magic = c.read_bytes(4)?;
    if magic != b"meta" {
        return Err(Error::BadInput("invalid metadata magic".into()));
    }
    let version = c.read_byte()?;
    if version != 14 && version != 15 {
        return Err(Error::BadInput(alloc::format!(
            "unsupported metadata version: {version}"
        )));
    }
    Ok(version)
}

pub fn postprocess_types(types: &mut [TypeDefOwned]) {
    // Vec<u8> → Bytes
    for i in 0..types.len() {
        if let TypeDefOwned::Sequence(inner) = types[i] {
            if matches!(types.get(inner as usize), Some(TypeDefOwned::U8)) {
                types[i] = TypeDefOwned::Bytes;
            }
        }
    }
    // BTreeMap: resolve inner Sequence→Tuple(K, V)
    for i in 0..types.len() {
        if let TypeDefOwned::Map(inner, _) = types[i] {
            if let Some(TypeDefOwned::Sequence(tuple_id)) = types.get(inner as usize) {
                let tuple_id = *tuple_id;
                if let Some(TypeDefOwned::Tuple(ids)) = types.get(tuple_id as usize) {
                    if ids.len() == 2 {
                        types[i] = TypeDefOwned::Map(ids[0], ids[1]);
                    }
                }
            }
        }
    }
}

/// Collect only storage + constant type IDs (no calls/events/extrinsic).
/// This avoids pulling in RuntimeCall/RuntimeEvent which transitively
/// reference every pallet's types — too much for memory-constrained targets.
pub fn collect_storage_type_ids(pallets: &[RawPallet]) -> Vec<u32> {
    let mut ids = Vec::new();
    for p in pallets {
        if let Some(ref s) = p.storage {
            for e in &s.entries {
                match &e.ty {
                    RawStorageType::Plain(t) => ids.push(*t),
                    RawStorageType::Map { key, value, .. } => {
                        ids.push(*key);
                        ids.push(*value);
                    }
                }
            }
        }
        for c in &p.constants {
            ids.push(c.ty);
        }
    }
    ids
}

pub fn collect_pallet_type_ids(pallets: &[RawPallet], extrinsic: &RawExtrinsic) -> Vec<u32> {
    let mut ids = Vec::new();
    for p in pallets {
        if let Some(ty) = p.calls_ty {
            ids.push(ty);
        }
        if let Some(ref s) = p.storage {
            for e in &s.entries {
                match &e.ty {
                    RawStorageType::Plain(t) => ids.push(*t),
                    RawStorageType::Map { key, value, .. } => {
                        ids.push(*key);
                        ids.push(*value);
                    }
                }
            }
        }
        for c in &p.constants {
            ids.push(c.ty);
        }
    }
    if let Some(addr) = extrinsic.address_ty {
        ids.push(addr);
    }
    if let Some(sig) = extrinsic.signature_ty {
        ids.push(sig);
    }
    for ext in &extrinsic.extensions {
        ids.push(ext.ty);
        ids.push(ext.additional_signed);
    }
    ids
}

pub fn remap_pallet_ids(pallets: Vec<RawPallet>, remap: &dyn Fn(u32) -> u32) -> Vec<RawPallet> {
    pallets
        .into_iter()
        .map(|mut p| {
            p.calls_ty = p.calls_ty.map(remap);
            if let Some(ref mut s) = p.storage {
                for e in &mut s.entries {
                    match &mut e.ty {
                        RawStorageType::Plain(t) => *t = remap(*t),
                        RawStorageType::Map { key, value, .. } => {
                            *key = remap(*key);
                            *value = remap(*value);
                        }
                    }
                }
            }
            for c in &mut p.constants {
                c.ty = remap(c.ty);
            }
            p
        })
        .collect()
}

pub fn remap_extrinsic_ids(mut ext: RawExtrinsic, remap: &dyn Fn(u32) -> u32) -> RawExtrinsic {
    ext.address_ty = ext.address_ty.map(remap);
    ext.signature_ty = ext.signature_ty.map(remap);
    for e in &mut ext.extensions {
        e.ty = remap(e.ty);
        e.additional_signed = remap(e.additional_signed);
    }
    ext
}

// --- Type decode/skip ---

fn decode_portable_type(c: &mut Cursor) -> Result<(u32, String, TypeDefOwned), Error> {
    let id = c.read_compact_u32()?;

    // path: last segment = short name
    let path_len = c.read_compact_u32()?;
    let mut short_name = String::new();
    for i in 0..path_len {
        if i == path_len - 1 {
            short_name = c.read_string()?;
        } else {
            c.skip_string()?;
        }
    }

    let is_btreemap = short_name == "BTreeMap";

    // type_params
    let param_count = c.read_compact_u32()?;
    for _ in 0..param_count {
        c.skip_string()?;
        c.skip_option_compact_u32()?;
    }

    let mut td = decode_type_def(c, is_btreemap)?;

    if let TypeDefOwned::Variant(ref mut vdef) = td {
        vdef.name = short_name.clone();
    }

    // docs
    c.skip_vec_string()?;

    Ok((id, short_name, td))
}

fn skip_portable_type(c: &mut Cursor) -> Result<(), Error> {
    c.skip_vec_string()?; // path
    let param_count = c.read_compact_u32()?;
    for _ in 0..param_count {
        c.skip_string()?;
        c.skip_option_compact_u32()?;
    }
    skip_type_def(c)?;
    c.skip_vec_string()?; // docs
    Ok(())
}

fn decode_type_def(c: &mut Cursor, is_btreemap: bool) -> Result<TypeDefOwned, Error> {
    let idx = c.read_byte()?;
    match idx {
        0 => decode_composite(c, is_btreemap),
        1 => decode_variant(c),
        2 => Ok(TypeDefOwned::Sequence(c.read_compact_u32()?)),
        3 => {
            let len = c.read_u32_le()?;
            Ok(TypeDefOwned::Array(c.read_compact_u32()?, len))
        }
        4 => {
            let count = c.read_compact_u32()?;
            let mut ids = Vec::with_capacity(count as usize);
            for _ in 0..count {
                ids.push(c.read_compact_u32()?);
            }
            Ok(TypeDefOwned::Tuple(ids))
        }
        5 => decode_primitive(c),
        6 => Ok(TypeDefOwned::Compact(c.read_compact_u32()?)),
        7 => Ok(TypeDefOwned::BitSequence(
            c.read_compact_u32()?,
            c.read_compact_u32()?,
        )),
        _ => Err(Error::BadInput("unknown TypeDef variant".into())),
    }
}

fn decode_composite(c: &mut Cursor, is_btreemap: bool) -> Result<TypeDefOwned, Error> {
    let count = c.read_compact_u32()?;
    let mut named = Vec::new();
    let mut unnamed = Vec::new();
    let mut has_names = true;
    for _ in 0..count {
        let name = if c.read_byte()? != 0 {
            Some(c.read_string()?)
        } else {
            has_names = false;
            None
        };
        let ty = c.read_compact_u32()?;
        if c.read_byte()? != 0 {
            c.skip_string()?;
        } // type_name
        c.skip_vec_string()?; // docs
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

fn decode_variant(c: &mut Cursor) -> Result<TypeDefOwned, Error> {
    let count = c.read_compact_u32()?;
    let mut variants = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = c.read_string()?;
        let field_count = c.read_compact_u32()?;
        let mut named = Vec::new();
        let mut unnamed = Vec::new();
        let mut has_names = true;
        for _ in 0..field_count {
            let fname = if c.read_byte()? != 0 {
                Some(c.read_string()?)
            } else {
                has_names = false;
                None
            };
            let ty = c.read_compact_u32()?;
            if c.read_byte()? != 0 {
                c.skip_string()?;
            }
            c.skip_vec_string()?;
            if let Some(fname) = fname {
                named.push(FieldOwned { name: fname, ty });
            }
            unnamed.push(ty);
        }
        let index = c.read_byte()?;
        c.skip_vec_string()?; // docs

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

fn decode_primitive(c: &mut Cursor) -> Result<TypeDefOwned, Error> {
    Ok(match c.read_byte()? {
        0 => TypeDefOwned::Bool,
        1 => TypeDefOwned::Char,
        2 => TypeDefOwned::Str,
        3 => TypeDefOwned::U8,
        4 => TypeDefOwned::U16,
        5 => TypeDefOwned::U32,
        6 => TypeDefOwned::U64,
        7 => TypeDefOwned::U128,
        8 => TypeDefOwned::U128, // U256 mapped to U128
        9 => TypeDefOwned::I8,
        10 => TypeDefOwned::I16,
        11 => TypeDefOwned::I32,
        12 => TypeDefOwned::I64,
        13 => TypeDefOwned::I128,
        _ => return Err(Error::BadInput("unknown primitive kind".into())),
    })
}

fn skip_type_def(c: &mut Cursor) -> Result<(), Error> {
    match c.read_byte()? {
        0 => {
            // Composite
            let count = c.read_compact_u32()?;
            for _ in 0..count {
                if c.read_byte()? != 0 {
                    c.skip_string()?;
                } // name
                c.read_compact_u32()?; // ty
                if c.read_byte()? != 0 {
                    c.skip_string()?;
                } // type_name
                c.skip_vec_string()?; // docs
            }
        }
        1 => {
            // Variant
            let count = c.read_compact_u32()?;
            for _ in 0..count {
                c.skip_string()?;
                let fc = c.read_compact_u32()?;
                for _ in 0..fc {
                    if c.read_byte()? != 0 {
                        c.skip_string()?;
                    }
                    c.read_compact_u32()?;
                    if c.read_byte()? != 0 {
                        c.skip_string()?;
                    }
                    c.skip_vec_string()?;
                }
                c.read_byte()?;
                c.skip_vec_string()?;
            }
        }
        2 => {
            c.read_compact_u32()?;
        }
        3 => {
            c.read_u32_le()?;
            c.read_compact_u32()?;
        }
        4 => {
            let n = c.read_compact_u32()?;
            for _ in 0..n {
                c.read_compact_u32()?;
            }
        }
        5 => {
            c.read_byte()?;
        }
        6 => {
            c.read_compact_u32()?;
        }
        7 => {
            c.read_compact_u32()?;
            c.read_compact_u32()?;
        }
        _ => return Err(Error::BadInput("unknown TypeDef variant".into())),
    }
    Ok(())
}

// --- Type reference collection and remapping ---

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

pub fn remap_type_ids(td: &mut TypeDefOwned, id_map: &[Option<u32>]) {
    fn r(id: &mut u32, map: &[Option<u32>]) {
        if let Some(new) = map.get(*id as usize).copied().flatten() {
            *id = new;
        }
    }
    match td {
        TypeDefOwned::Sequence(id)
        | TypeDefOwned::StructNewType(id)
        | TypeDefOwned::Compact(id) => r(id, id_map),
        TypeDefOwned::Map(k, v) | TypeDefOwned::BitSequence(k, v) => {
            r(k, id_map);
            r(v, id_map);
        }
        TypeDefOwned::Array(id, _) => r(id, id_map),
        TypeDefOwned::Tuple(ids) | TypeDefOwned::StructTuple(ids) => {
            for id in ids {
                r(id, id_map);
            }
        }
        TypeDefOwned::Struct(fields) => {
            for f in fields {
                r(&mut f.ty, id_map);
            }
        }
        TypeDefOwned::Variant(vdef) => {
            for v in &mut vdef.variants {
                match &mut v.fields {
                    FieldsOwned::NewType(id) => r(id, id_map),
                    FieldsOwned::Tuple(ids) => {
                        for id in ids {
                            r(id, id_map);
                        }
                    }
                    FieldsOwned::Struct(fields) => {
                        for f in fields {
                            r(&mut f.ty, id_map);
                        }
                    }
                    FieldsOwned::Unit => {}
                }
            }
        }
        _ => {}
    }
}

// --- Pallet/extrinsic decode ---

fn decode_pallet(c: &mut Cursor, version: u8) -> Result<RawPallet, Error> {
    let name = c.read_string()?;

    let storage = if c.read_byte()? != 0 {
        let prefix = c.read_string()?;
        let count = c.read_compact_u32()?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(decode_storage_entry(c)?);
        }
        Some(RawStorage { prefix, entries })
    } else {
        None
    };

    let calls_ty = if c.read_byte()? != 0 {
        Some(c.read_compact_u32()?)
    } else {
        None
    };
    if c.read_byte()? != 0 {
        c.read_compact_u32()?;
    } // event

    let const_count = c.read_compact_u32()?;
    let mut constants = Vec::with_capacity(const_count as usize);
    for _ in 0..const_count {
        let name = c.read_string()?;
        let ty = c.read_compact_u32()?;
        let value_len = c.read_compact_u32()? as usize;
        let value = c.read_bytes(value_len)?.to_vec();
        c.skip_vec_string()?;
        constants.push(RawConstant { name, ty, value });
    }

    if c.read_byte()? != 0 {
        c.read_compact_u32()?;
    } // error
    let index = c.read_byte()?;
    if version >= 15 {
        c.skip_vec_string()?; // docs (V15+ only)
    }

    Ok(RawPallet {
        name,
        index,
        calls_ty,
        storage,
        constants,
    })
}

fn decode_storage_entry(c: &mut Cursor) -> Result<RawStorageEntry, Error> {
    let name = c.read_string()?;
    let modifier = c.read_byte()?;
    let ty = match c.read_byte()? {
        0 => RawStorageType::Plain(c.read_compact_u32()?),
        1 => {
            let hc = c.read_compact_u32()?;
            let mut hashers = Vec::with_capacity(hc as usize);
            for _ in 0..hc {
                hashers.push(c.read_byte()?);
            }
            RawStorageType::Map {
                hashers,
                key: c.read_compact_u32()?,
                value: c.read_compact_u32()?,
            }
        }
        _ => return Err(Error::BadInput("unknown storage entry type".into())),
    };
    let default_len = c.read_compact_u32()? as usize;
    let default = c.read_bytes(default_len)?.to_vec();
    c.skip_vec_string()?;
    Ok(RawStorageEntry {
        name,
        modifier,
        ty,
        default,
    })
}

fn decode_extrinsic(c: &mut Cursor, version: u8) -> Result<RawExtrinsic, Error> {
    if version == 14 {
        let _ty = c.read_compact_u32()?;
        let ext_version = c.read_byte()?;
        let count = c.read_compact_u32()?;
        let mut extensions = Vec::with_capacity(count as usize);
        for _ in 0..count {
            extensions.push(RawExtension {
                identifier: c.read_string()?,
                ty: c.read_compact_u32()?,
                additional_signed: c.read_compact_u32()?,
            });
        }
        Ok(RawExtrinsic {
            version: ext_version,
            address_ty: None,
            signature_ty: None,
            extensions,
        })
    } else {
        // V15: version, address_ty, call_ty, signature_ty, extra_ty, extensions
        let ext_version = c.read_byte()?;
        let address_ty = c.read_compact_u32()?;
        let _call_ty = c.read_compact_u32()?;
        let signature_ty = c.read_compact_u32()?;
        let _extra_ty = c.read_compact_u32()?;
        let count = c.read_compact_u32()?;
        let mut extensions = Vec::with_capacity(count as usize);
        for _ in 0..count {
            extensions.push(RawExtension {
                identifier: c.read_string()?,
                ty: c.read_compact_u32()?,
                additional_signed: c.read_compact_u32()?,
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

#[cfg(test)]
mod tests {
    use super::*;

    const KREIVO_META: &[u8] = include_bytes!("../../tests/fixtures/kreivo.scale");

    #[test]
    fn decode_full_kreivo() {
        let raw = decode_metadata(KREIVO_META).unwrap();
        assert_eq!(raw.types.len(), 764);
        assert_eq!(raw.pallets.len(), 44);
        assert!(raw.pallets.iter().any(|p| p.name == "System"));
        assert!(raw.pallets.iter().any(|p| p.name == "Balances"));
    }

    #[test]
    fn decode_filtered_has_fewer_types() {
        let full = decode_metadata(KREIVO_META).unwrap();
        let filtered = decode_metadata_filtered(KREIVO_META, &["Balances"]).unwrap();

        assert!(filtered.types.len() < full.types.len());
        assert_eq!(filtered.pallets.len(), 2); // System + Balances
        assert!(filtered.pallets.iter().any(|p| p.name == "System"));
        assert!(filtered.pallets.iter().any(|p| p.name == "Balances"));
    }

    #[test]
    fn filtered_system_always_included() {
        let raw = decode_metadata_filtered(KREIVO_META, &["Timestamp"]).unwrap();
        assert!(raw.pallets.iter().any(|p| p.name == "System"));
        assert!(raw.pallets.iter().any(|p| p.name == "Timestamp"));
    }

    #[test]
    fn filtered_types_are_valid() {
        let raw = decode_metadata_filtered(KREIVO_META, &["Balances"]).unwrap();
        let registry = Registry::new(raw.types);

        // All pallet TypeIds should resolve
        for p in &raw.pallets {
            if let Some(ty) = p.calls_ty {
                assert!(
                    registry.resolve(ty).is_some(),
                    "calls_ty {} resolves for {}",
                    ty,
                    p.name
                );
            }
            for c in &p.constants {
                assert!(
                    registry.resolve(c.ty).is_some(),
                    "constant {} ty resolves",
                    c.name
                );
            }
        }
    }

    #[test]
    fn extrinsic_extensions_decoded() {
        let raw = decode_metadata(KREIVO_META).unwrap();
        assert!(!raw.extrinsic.extensions.is_empty());
        assert!(raw
            .extrinsic
            .extensions
            .iter()
            .any(|e| e.identifier.contains("CheckNonce") || e.identifier.contains("Nonce")));
    }

    #[test]
    fn storage_entries_decoded() {
        let raw = decode_metadata(KREIVO_META).unwrap();
        let system = raw.pallets.iter().find(|p| p.name == "System").unwrap();
        let storage = system.storage.as_ref().unwrap();
        assert!(storage.entries.iter().any(|e| e.name == "Account"));
        assert!(storage.entries.iter().any(|e| e.name == "Number"));
    }
}

// Test: verify filtered registry size for streaming metadata budget planning
#[cfg(test)]
mod streaming_size_tests {
    use super::*;
    use alloc::collections::BTreeSet;

    const KREIVO_META: &[u8] = include_bytes!("../../tests/fixtures/kreivo.scale");

    #[test]
    fn measure_filtered_registry_storage_only() {
        let raw = decode_metadata(KREIVO_META).unwrap();
        let pallets: Vec<_> = raw
            .pallets
            .into_iter()
            .filter(|p| p.name == "System" || p.name == "CollatorSelection")
            .collect();

        let root = collect_storage_type_ids(&pallets);
        println!("Root storage type IDs: {} entries", root.len());

        // Transitive closure
        let mut needed = BTreeSet::new();
        let mut queue = root;
        while let Some(id) = queue.pop() {
            if !needed.insert(id) {
                continue;
            }
            if let Some(td) = raw.types.get(id as usize) {
                collect_type_refs(td, &mut queue);
            }
        }
        println!("Transitive types needed: {}", needed.len());

        // Build filtered types
        let mut id_map: Vec<Option<u32>> = vec![None; raw.types.len()];
        let mut filtered = Vec::new();
        for &old_id in &needed {
            if (old_id as usize) < id_map.len() {
                id_map[old_id as usize] = Some(filtered.len() as u32);
            }
            if let Some(td) = raw.types.get(old_id as usize) {
                let mut td = td.clone();
                remap_type_ids(&mut td, &id_map);
                filtered.push(td);
            }
        }
        postprocess_types(&mut filtered);

        // Measure type sizes
        let mut total_strings = 0usize;
        let mut total_fields = 0usize;
        let mut total_variants = 0usize;
        let mut total_type_ids = 0usize;
        for td in &filtered {
            match td {
                TypeDefOwned::Struct(fields) => {
                    total_fields += fields.len();
                    for f in fields {
                        total_strings += f.name.len();
                    }
                }
                TypeDefOwned::Variant(vdef) => {
                    total_variants += vdef.variants.len();
                    total_strings += vdef.name.len();
                    for v in &vdef.variants {
                        total_strings += v.name.len();
                        match &v.fields {
                            FieldsOwned::Struct(fields) => {
                                total_fields += fields.len();
                                for f in fields {
                                    total_strings += f.name.len();
                                }
                            }
                            FieldsOwned::Tuple(ids) => total_type_ids += ids.len(),
                            _ => {}
                        }
                    }
                }
                TypeDefOwned::Tuple(ids) | TypeDefOwned::StructTuple(ids) => {
                    total_type_ids += ids.len();
                }
                _ => {}
            }
        }

        let tdi_size = filtered.len() * 12; // approx TDI enum size
        let fields_size = total_fields * 8; // FI = StrId + TypeId
        let variants_size = total_variants * 12; // VI = u8 + StrId + VFI
        let type_ids_size = total_type_ids * 4;
        let str_idx_size = (total_fields + total_variants + filtered.len()) * 6; // (u32, u16) per string
        let total =
            tdi_size + fields_size + variants_size + type_ids_size + total_strings + str_idx_size;

        println!("Filtered registry estimate:");
        println!(
            "  types:    {} entries, ~{} bytes",
            filtered.len(),
            tdi_size
        );
        println!("  fields:   {}, ~{} bytes", total_fields, fields_size);
        println!("  variants: {}, ~{} bytes", total_variants, variants_size);
        println!("  type_ids: {}, ~{} bytes", total_type_ids, type_ids_size);
        println!("  strings:  {} bytes", total_strings);
        println!("  str_idx:  ~{} bytes", str_idx_size);
        println!("  TOTAL:    ~{} bytes (~{}KB)", total, total / 1024);
    }
}
