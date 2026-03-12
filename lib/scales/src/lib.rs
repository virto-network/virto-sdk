#![cfg_attr(not(feature = "std"), no_std)]
//!
//! # Scales
//!
//! Dynamic SCALE Serialization using `scale-info` type information.

#[macro_use]
extern crate alloc;

#[cfg(feature = "scale-info")]
pub mod compress;
pub mod registry;
#[cfg(feature = "experimental-serializer")]
mod serializer;
mod value;

pub use bytes::Bytes;
pub use registry::{Registry, TypeDef, TypeId};
#[cfg(feature = "json")]
pub use serde_json::Value as JsonValue;
#[cfg(feature = "experimental-serializer")]
pub use serializer::{to_bytes, to_bytes_with_info, to_vec, to_vec_with_info, Serializer};
#[cfg(all(feature = "json", feature = "experimental-serializer"))]
pub use serializer::{to_bytes_from_iter, to_vec_from_iter};
pub use value::Value;

mod prelude {
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
}

// adapted from https://github.com/paritytech/parity-scale-codec/blob/master/src/compact.rs#L336
#[allow(clippy::all)]
pub(crate) fn compact_encode(n: u128, mut dest: impl bytes::BufMut) {
    match n {
        0..=0b0011_1111 => dest.put_u8((n as u8) << 2),
        0..=0b0011_1111_1111_1111 => dest.put_u16_le(((n as u16) << 2) | 0b01),
        0..=0b0011_1111_1111_1111_1111_1111_1111_1111 => dest.put_u32_le(((n as u32) << 2) | 0b10),
        _ => {
            let bytes_needed = 16 - n.leading_zeros() / 8;
            assert!(bytes_needed >= 4);
            dest.put_u8(0b11 + ((bytes_needed - 4) << 2) as u8);
            let mut v = n;
            for _ in 0..bytes_needed {
                dest.put_u8(v as u8);
                v >>= 8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use codec::{Compact, Encode};

    #[test]
    fn compact_encode_single_byte() {
        for v in [0u32, 1, 42, 63] {
            let expected = Compact(v).encode();
            let mut out = Vec::new();
            compact_encode(v as u128, &mut out);
            assert_eq!(out, expected, "mismatch for {v}");
        }
    }

    #[test]
    fn compact_encode_two_bytes() {
        for v in [64u32, 255, 1000, 16383] {
            let expected = Compact(v).encode();
            let mut out = Vec::new();
            compact_encode(v as u128, &mut out);
            assert_eq!(out, expected, "mismatch for {v}");
        }
    }

    #[test]
    fn compact_encode_four_bytes() {
        for v in [16384u32, 65535, 1_000_000, (1 << 30) - 1] {
            let expected = Compact(v).encode();
            let mut out = Vec::new();
            compact_encode(v as u128, &mut out);
            assert_eq!(out, expected, "mismatch for {v}");
        }
    }

    #[test]
    fn compact_encode_big_u32() {
        for v in [1u32 << 30, u32::MAX] {
            let expected = Compact(v).encode();
            let mut out = Vec::new();
            compact_encode(v as u128, &mut out);
            assert_eq!(out, expected, "mismatch for {v}");
        }
    }

    #[test]
    fn compact_encode_u64() {
        for v in [u32::MAX as u64 + 1, 1_000_000_000_000, u64::MAX] {
            let expected = Compact(v).encode();
            let mut out = Vec::new();
            compact_encode(v as u128, &mut out);
            assert_eq!(out, expected, "mismatch for {v}");
        }
    }

    #[test]
    fn compact_encode_u128() {
        for v in [u64::MAX as u128 + 1, u128::MAX] {
            let expected = Compact(v).encode();
            let mut out = Vec::new();
            compact_encode(v, &mut out);
            assert_eq!(out, expected, "mismatch for {v}");
        }
    }

    #[test]
    fn registry_size_reduction() {
        use codec::Decode;
        use scale_info::PortableRegistry;

        let raw = include_bytes!("registry.bin");
        let portable = PortableRegistry::decode(&mut &raw[..]).expect("decode");
        let compressed = compress::compress(&portable);

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
                registry::TypeDef::Bool
                | registry::TypeDef::U8
                | registry::TypeDef::U16
                | registry::TypeDef::U32
                | registry::TypeDef::U64
                | registry::TypeDef::U128
                | registry::TypeDef::I8
                | registry::TypeDef::I16
                | registry::TypeDef::I32
                | registry::TypeDef::I64
                | registry::TypeDef::I128
                | registry::TypeDef::Char
                | registry::TypeDef::Str
                | registry::TypeDef::Bytes
                | registry::TypeDef::StructUnit => {}
                registry::TypeDef::Sequence(id)
                | registry::TypeDef::StructNewType(id)
                | registry::TypeDef::Compact(id) => {
                    compressed_wire += 4;
                    let _ = id;
                }
                registry::TypeDef::Map(_, _) | registry::TypeDef::BitSequence(_, _) => {
                    compressed_wire += 8;
                }
                registry::TypeDef::Array(_, _) => {
                    compressed_wire += 8;
                }
                registry::TypeDef::Tuple(ids) | registry::TypeDef::StructTuple(ids) => {
                    compressed_wire += 1 + ids.len() * 4; // compact len + ids
                }
                registry::TypeDef::Struct(fields) => {
                    compressed_wire += 1; // compact len
                    for f in fields {
                        compressed_wire += 1 + f.name.len() + 4; // compact str len + str + ty_id
                    }
                }
                registry::TypeDef::Variant(vdef) => {
                    compressed_wire += 1 + vdef.name.len(); // compact len + name
                    compressed_wire += 1; // compact variants len
                    for v in &vdef.variants {
                        compressed_wire += 1; // index
                        compressed_wire += 1 + v.name.len(); // compact len + name
                        compressed_wire += 1; // fields discriminant
                        match &v.fields {
                            registry::Fields::Unit => {}
                            registry::Fields::NewType(_) => {
                                compressed_wire += 4;
                            }
                            registry::Fields::Tuple(ids) => {
                                compressed_wire += 1 + ids.len() * 4;
                            }
                            registry::Fields::Struct(fields) => {
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
}
