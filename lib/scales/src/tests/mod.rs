mod serializer;
#[cfg(feature = "text")]
mod textfmt;
mod value;

use crate::compress;
use crate::registry::*;
use alloc::vec::Vec;
use codec::Encode;
use scale_info::{meta_type, PortableRegistry, Registry as SiRegistry, TypeInfo};

fn register<T>(_ty: &T) -> (TypeId, Registry)
where
    T: TypeInfo + 'static,
{
    let mut reg = SiRegistry::new();
    let sym = reg.register_type(&meta_type::<T>());
    let portable: PortableRegistry = reg.into();
    (sym.id, compress::compress(&portable).expect("compress"))
}

#[test]
fn compact_encode_single_byte() {
    use codec::Compact;
    for v in [0u32, 1, 42, 63] {
        let expected = Compact(v).encode();
        let mut out = Vec::new();
        crate::compact_encode(v as u128, &mut out);
        assert_eq!(out, expected, "mismatch for {v}");
    }
}

#[test]
fn compact_encode_two_bytes() {
    use codec::Compact;
    for v in [64u32, 255, 1000, 16383] {
        let expected = Compact(v).encode();
        let mut out = Vec::new();
        crate::compact_encode(v as u128, &mut out);
        assert_eq!(out, expected, "mismatch for {v}");
    }
}

#[test]
fn compact_encode_four_bytes() {
    use codec::Compact;
    for v in [16384u32, 65535, 1_000_000, (1 << 30) - 1] {
        let expected = Compact(v).encode();
        let mut out = Vec::new();
        crate::compact_encode(v as u128, &mut out);
        assert_eq!(out, expected, "mismatch for {v}");
    }
}

#[test]
fn compact_encode_big_u32() {
    use codec::Compact;
    for v in [1u32 << 30, u32::MAX] {
        let expected = Compact(v).encode();
        let mut out = Vec::new();
        crate::compact_encode(v as u128, &mut out);
        assert_eq!(out, expected, "mismatch for {v}");
    }
}

#[test]
fn compact_encode_u64() {
    use codec::Compact;
    for v in [u32::MAX as u64 + 1, 1_000_000_000_000, u64::MAX] {
        let expected = Compact(v).encode();
        let mut out = Vec::new();
        crate::compact_encode(v as u128, &mut out);
        assert_eq!(out, expected, "mismatch for {v}");
    }
}

#[test]
fn compact_encode_u128() {
    use codec::Compact;
    for v in [u64::MAX as u128 + 1, u128::MAX] {
        let expected = Compact(v).encode();
        let mut out = Vec::new();
        crate::compact_encode(v, &mut out);
        assert_eq!(out, expected, "mismatch for {v}");
    }
}

#[test]
fn registry_size_reduction() {
    use codec::Decode;

    let raw = include_bytes!("../registry.bin");
    let portable = PortableRegistry::decode(&mut &raw[..]).expect("decode");
    let compressed = compress::compress(&portable).expect("compress");

    let encoded_portable = portable.encode();
    let num_types = portable.types.len();

    // Break down PortableRegistry content
    let mut paths_bytes = 0usize;
    let mut docs_bytes = 0usize;
    let mut params_bytes = 0usize;
    for pt in &portable.types {
        for seg in &pt.ty.path.segments {
            paths_bytes += seg.len();
        }
        for doc in &pt.ty.docs {
            docs_bytes += doc.len();
        }
        for tp in &pt.ty.type_params {
            params_bytes += tp.name.len();
        }
    }
    let structure_bytes = encoded_portable.len() - paths_bytes - docs_bytes - params_bytes;

    // Estimate serialized size of compressed registry
    // (what you'd need to transmit/store)
    let mut compressed_wire = 0usize;
    for i in 0..num_types {
        let ty = compressed.resolve(i as u32).unwrap();
        compressed_wire += 1; // discriminant
        match ty {
            TypeDef::Bool
            | TypeDef::U8
            | TypeDef::U16
            | TypeDef::U32
            | TypeDef::U64
            | TypeDef::U128
            | TypeDef::I8
            | TypeDef::I16
            | TypeDef::I32
            | TypeDef::I64
            | TypeDef::I128
            | TypeDef::Char
            | TypeDef::Str
            | TypeDef::Bytes
            | TypeDef::StructUnit => {}
            TypeDef::Sequence(id) | TypeDef::StructNewType(id) | TypeDef::Compact(id) => {
                compressed_wire += 4;
                let _ = id;
            }
            TypeDef::Map(_, _) | TypeDef::BitSequence(_, _) => {
                compressed_wire += 8;
            }
            TypeDef::Array(_, _) => {
                compressed_wire += 8;
            }
            TypeDef::Tuple(ids) | TypeDef::StructTuple(ids) => {
                compressed_wire += 1 + ids.len() * 4; // compact len + ids
            }
            TypeDef::Struct(fields) => {
                compressed_wire += 1; // compact len
                for f in fields {
                    compressed_wire += 1 + f.name.len() + 4; // compact str len + str + ty_id
                }
            }
            TypeDef::Variant(vdef) => {
                compressed_wire += 1 + vdef.name.len(); // compact len + name
                compressed_wire += 1; // compact variants len
                for v in &vdef.variants {
                    compressed_wire += 1; // index
                    compressed_wire += 1 + v.name.len(); // compact len + name
                    compressed_wire += 1; // fields discriminant
                    match &v.fields {
                        Fields::Unit => {}
                        Fields::NewType(_) => {
                            compressed_wire += 4;
                        }
                        Fields::Tuple(ids) => {
                            compressed_wire += 1 + ids.len() * 4;
                        }
                        Fields::Struct(fields) => {
                            compressed_wire += 1;
                            for f in fields {
                                compressed_wire += 1 + f.name.len() + 4;
                            }
                        }
                    }
                }
            }
        }
    }

    eprintln!("=== Registry Size Comparison (real Substrate registry) ===");
    eprintln!("Types: {num_types}");
    eprintln!();
    eprintln!(
        "PortableRegistry (SCALE encoded): {} bytes",
        encoded_portable.len()
    );
    eprintln!("  paths:      {paths_bytes:>6} bytes");
    eprintln!("  docs:       {docs_bytes:>6} bytes");
    eprintln!("  params:     {params_bytes:>6} bytes");
    eprintln!("  structure:  {structure_bytes:>6} bytes");
    eprintln!();
    eprintln!("Compressed Registry (wire est.):  {compressed_wire} bytes");
    eprintln!();
    let reduction = (1.0 - compressed_wire as f64 / encoded_portable.len() as f64) * 100.0;
    eprintln!("Wire size reduction: ~{reduction:.0}%");

    assert!(
        compressed_wire < encoded_portable.len(),
        "compressed should be smaller"
    );
}
