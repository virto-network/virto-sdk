//! Extrinsic decoder — parses raw Substrate extrinsic bytes into structured parts.
//!
//! Decodes the frame-specific envelope (version, address, signature, extensions, call)
//! without needing `parity-scale-codec`. The call body can then be decoded with
//! [`Value`](crate::Value) using the registry.

use alloc::vec::Vec;

use super::cursor::Cursor;
use crate::registry::{Registry, TypeDef, TypeId};
use crate::Error;

/// A decoded extrinsic envelope.
///
/// ## Wire format (V4)
/// ```text
/// compact_len | version_byte | [address | signature | extensions] | pallet_idx | call_idx | call_args
/// ```
/// - `version_byte`: bit 7 = signed, bits 0-6 = version (4)
/// - `extensions`: concatenated SCALE-encoded "extra" data from each signed/transaction extension
///
/// The call args can be decoded with `Value::new(ext.call_args, calls_type_id, &registry)`.
pub struct Extrinsic<'a> {
    /// True if the extrinsic is signed.
    pub signed: bool,
    /// Extrinsic version (4 for current Substrate chains).
    pub version: u8,
    /// Address bytes (if signed). Decode with metadata's address type.
    pub address: Option<&'a [u8]>,
    /// Signature bytes (if signed). Decode with metadata's signature type.
    pub signature: Option<&'a [u8]>,
    /// Transaction extension data (if signed). Concatenated SCALE for each extension's "extra" type.
    pub extensions: Option<&'a [u8]>,
    /// Pallet index.
    pub pallet_index: u8,
    /// Call variant index within the pallet.
    pub call_index: u8,
    /// Raw SCALE-encoded call arguments (after pallet + call index).
    pub call_args: &'a [u8],
}

/// Decode a single extrinsic from raw SCALE bytes.
///
/// The input should be a single extrinsic (with compact length prefix),
/// as returned by `chainHead_v1_body` or found in a block body.
///
/// To decode the call args, use `Value::new(ext.call_args, call_type_id, &registry)`.
pub fn decode_extrinsic<'a>(
    data: &'a [u8],
    registry: &Registry,
    address_ty: Option<TypeId>,
    signature_ty: Option<TypeId>,
    extension_tys: &[(TypeId, TypeId)], // (extra_ty, additional_signed_ty) — only extra is in the extrinsic
) -> Result<Extrinsic<'a>, Error> {
    let mut c = Cursor::new(data);

    // Compact length prefix
    let _len = c.read_compact_u32()?;

    // Version byte: bit 7 = signed flag, bits 0-6 = version
    let version_byte = c.read_byte()?;
    let signed = version_byte & 0x80 != 0;
    let version = version_byte & 0x7f;

    let (address, signature, extensions) = if signed {
        // Address
        let addr_start = c.pos;
        if let Some(addr_ty) = address_ty {
            skip_type(&mut c, addr_ty, registry)?;
        }
        let address = &data[addr_start..c.pos];

        // Signature
        let sig_start = c.pos;
        if let Some(sig_ty) = signature_ty {
            skip_type(&mut c, sig_ty, registry)?;
        }
        let signature = &data[sig_start..c.pos];

        // Transaction extensions ("extra" data — the implicit/additional_signed is NOT in the body)
        let ext_start = c.pos;
        for &(ext_ty, _) in extension_tys {
            skip_type(&mut c, ext_ty, registry)?;
        }
        let extensions = &data[ext_start..c.pos];

        (Some(address), Some(signature), Some(extensions))
    } else {
        (None, None, None)
    };

    // Call: pallet_index(u8) + call_index(u8) + args
    let pallet_index = c.read_byte()?;
    let call_index = c.read_byte()?;
    let call_args = &data[c.pos..];

    Ok(Extrinsic {
        signed,
        version,
        address,
        signature,
        extensions,
        pallet_index,
        call_index,
        call_args,
    })
}

/// Decode all extrinsics from a block body (Vec of extrinsics).
///
/// `body` is the SCALE-encoded `Vec<Extrinsic>` (compact length prefix + N extrinsics).
pub fn decode_block_extrinsics<'a>(
    body: &'a [u8],
    registry: &Registry,
    address_ty: Option<TypeId>,
    signature_ty: Option<TypeId>,
    extension_tys: &[(TypeId, TypeId)],
) -> Result<Vec<Extrinsic<'a>>, Error> {
    let mut c = Cursor::new(body);
    let count = c.read_compact_u32()?;
    let mut extrinsics = Vec::with_capacity(count as usize);

    for _ in 0..count {
        // Each extrinsic starts with a compact length
        let start = c.pos;
        let len = c.read_compact_u32()? as usize;
        let prefix_size = c.pos - start;
        let ext_data = &body[start..start + prefix_size + len];
        c.pos = start + prefix_size + len; // advance past this extrinsic

        extrinsics.push(decode_extrinsic(
            ext_data,
            registry,
            address_ty,
            signature_ty,
            extension_tys,
        )?);
    }

    Ok(extrinsics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::*;

    fn minimal_registry() -> Registry {
        // Types: 0=U8, 1=U32, 2=Bool, 3=Bytes, 4=Compact<U32>,
        // 5=Variant{None,Some(U32)} (for MultiAddress-like),
        // 6=Variant{Ed25519([u8;64]),Sr25519([u8;64])} (for MultiSignature-like)
        Registry::new(vec![
            TypeDefOwned::U8,                    // 0
            TypeDefOwned::U32,                   // 1
            TypeDefOwned::Bool,                  // 2
            TypeDefOwned::Bytes,                 // 3
            TypeDefOwned::Compact(1),            // 4: Compact<u32>
            TypeDefOwned::Variant(VariantDefOwned {   // 5: simple address enum
                name: "Address".into(),
                variants: vec![
                    VariantOwned { index: 0, name: "Id".into(), fields: FieldsOwned::NewType(3) },
                ],
            }),
            TypeDefOwned::Variant(VariantDefOwned {   // 6: signature enum
                name: "Signature".into(),
                variants: vec![
                    VariantOwned { index: 1, name: "Sr25519".into(), fields: FieldsOwned::NewType(3) },
                ],
            }),
        ])
    }

    #[test]
    fn decode_unsigned_extrinsic() {
        // Unsigned: compact_len || version(0x04) || pallet(1) || call(2) || arg(0x42)
        let body = [0x04, 1, 2, 0x42]; // version=4 unsigned, pallet=1, call=2, arg=0x42
        let compact_len = (body.len() as u8) << 2; // single-byte compact
        let data = [&[compact_len], &body[..]].concat();

        let registry = minimal_registry();
        let ext = decode_extrinsic(&data, &registry, None, None, &[]).unwrap();

        assert!(!ext.signed);
        assert_eq!(ext.version, 4);
        assert!(ext.address.is_none());
        assert!(ext.signature.is_none());
        assert_eq!(ext.pallet_index, 1);
        assert_eq!(ext.call_index, 2);
        assert_eq!(ext.call_args, &[0x42]);
    }

    #[test]
    fn decode_signed_extrinsic() {
        // Signed: compact_len || version(0x84) || address || signature || extra || call
        let mut body = vec![0x84]; // version=4, signed

        // Address: variant index 0 (Id) + bytes "abc" (compact 3 + 3 bytes)
        body.push(0x00); // variant 0
        body.push(0x0C); // compact(3)
        body.extend_from_slice(b"abc");

        // Signature: variant index 1 (Sr25519) + bytes "sig" (compact 3 + 3 bytes)
        body.push(0x01);
        body.push(0x0C);
        body.extend_from_slice(b"sig");

        // Extra: one Compact<u32> extension = nonce 42 (compact = 0xA8)
        body.push(0xA8); // compact(42)

        // Call: pallet 5, call 3, arg bytes "hi"
        body.push(5);
        body.push(3);
        body.extend_from_slice(b"hi");

        let compact_len = (body.len() as u8) << 2;
        let data = [&[compact_len], &body[..]].concat();

        let registry = minimal_registry();
        let extension_tys = [(4u32, 0u32)]; // one extension of type Compact<u32>

        let ext = decode_extrinsic(&data, &registry, Some(5), Some(6), &extension_tys).unwrap();

        assert!(ext.signed);
        assert_eq!(ext.version, 4);
        assert!(ext.address.is_some());
        assert!(ext.signature.is_some());
        assert_eq!(ext.pallet_index, 5);
        assert_eq!(ext.call_index, 3);
        assert_eq!(ext.call_args, b"hi");
    }

    #[test]
    fn skip_type_primitives() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05];
        let registry = minimal_registry();
        let mut c = Cursor::new(&data);

        skip_type(&mut c, 0, &registry).unwrap(); // U8: 1 byte
        assert_eq!(c.pos, 1);

        skip_type(&mut c, 1, &registry).unwrap(); // U32: 4 bytes
        assert_eq!(c.pos, 5);
    }

    #[test]
    fn skip_type_compact() {
        let data = [0xA8]; // compact(42) = 1 byte
        let registry = minimal_registry();
        let mut c = Cursor::new(&data);
        skip_type(&mut c, 4, &registry).unwrap(); // Compact<u32>
        assert_eq!(c.pos, 1);
    }

    #[test]
    fn skip_type_bytes() {
        let data = [0x0C, b'a', b'b', b'c', 0xFF]; // compact(3) + "abc" + sentinel
        let registry = minimal_registry();
        let mut c = Cursor::new(&data);
        skip_type(&mut c, 3, &registry).unwrap(); // Bytes
        assert_eq!(c.pos, 4); // 1 (compact) + 3 (bytes)
    }

    #[test]
    fn decode_block_body() {
        // Block body: Vec of 2 unsigned extrinsics
        let ext1 = {
            let body = [0x04, 0, 0, 0x11];
            let compact = (body.len() as u8) << 2;
            [&[compact], &body[..]].concat()
        };
        let ext2 = {
            let body = [0x04, 1, 1, 0x22, 0x33];
            let compact = (body.len() as u8) << 2;
            [&[compact], &body[..]].concat()
        };
        let mut block_body = vec![(2u8) << 2]; // compact(2) items
        block_body.extend(&ext1);
        block_body.extend(&ext2);

        let registry = minimal_registry();
        let exts = decode_block_extrinsics(&block_body, &registry, None, None, &[]).unwrap();

        assert_eq!(exts.len(), 2);
        assert_eq!(exts[0].pallet_index, 0);
        assert_eq!(exts[0].call_args, &[0x11]);
        assert_eq!(exts[1].pallet_index, 1);
        assert_eq!(exts[1].call_args, &[0x22, 0x33]);
    }
}

/// Skip past a SCALE-encoded value of the given type without allocating.
///
/// Advances the cursor past a value of `ty_id` using the registry
/// to determine the wire size. Useful for skipping over fields
/// we don't need to decode (e.g., signature, extensions in extrinsics).
pub fn skip_type(c: &mut Cursor, ty_id: TypeId, registry: &Registry) -> Result<(), Error> {
    let td = registry
        .resolve(ty_id)
        .ok_or(Error::BadInput("unresolved type in extrinsic".into()))?;

    match td {
        TypeDef::Bool | TypeDef::U8 | TypeDef::I8 => {
            c.read_byte()?;
        }
        TypeDef::U16 | TypeDef::I16 => {
            c.read_bytes(2)?;
        }
        TypeDef::U32 | TypeDef::I32 => {
            c.read_bytes(4)?;
        }
        TypeDef::U64 | TypeDef::I64 => {
            c.read_bytes(8)?;
        }
        TypeDef::U128 | TypeDef::I128 => {
            c.read_bytes(16)?;
        }
        TypeDef::Char => {
            c.read_bytes(4)?;
        }
        TypeDef::Str | TypeDef::Bytes => {
            c.skip_string()?;
        }
        TypeDef::Sequence(inner) => {
            let count = c.read_compact_u32()?;
            for _ in 0..count {
                skip_type(c, inner, registry)?;
            }
        }
        TypeDef::Array(inner, len) => {
            for _ in 0..len {
                skip_type(c, inner, registry)?;
            }
        }
        TypeDef::Tuple(ids) | TypeDef::StructTuple(ids) => {
            for id in ids {
                skip_type(c, *id, registry)?;
            }
        }
        TypeDef::Struct(fields) => {
            for f in fields {
                skip_type(c, f.ty, registry)?;
            }
        }
        TypeDef::StructUnit => {}
        TypeDef::StructNewType(inner) => {
            skip_type(c, inner, registry)?;
        }
        TypeDef::Variant(vdef) => {
            let index = c.read_byte()?;
            let variant = vdef
                .variant(index)
                .map_err(|_| Error::BadInput("invalid variant index in extrinsic".into()))?;
            match variant.fields() {
                crate::registry::Fields::Unit => {}
                crate::registry::Fields::NewType(id) => skip_type(c, id, registry)?,
                crate::registry::Fields::Tuple(ids) => {
                    for id in ids {
                        skip_type(c, *id, registry)?;
                    }
                }
                crate::registry::Fields::Struct(fields) => {
                    for f in fields {
                        skip_type(c, f.ty, registry)?;
                    }
                }
            }
        }
        TypeDef::Compact(_) => {
            c.read_compact_u32()?;
        }
        TypeDef::Map(k, v) => {
            let count = c.read_compact_u32()?;
            for _ in 0..count {
                skip_type(c, k, registry)?;
                skip_type(c, v, registry)?;
            }
        }
        TypeDef::BitSequence(_, _) => {
            // BitVec: compact length in bits, then ceil(bits/8) bytes
            let bits = c.read_compact_u32()?;
            let byte_count = (bits as usize + 7) / 8;
            c.read_bytes(byte_count)?;
        }
    }
    Ok(())
}
