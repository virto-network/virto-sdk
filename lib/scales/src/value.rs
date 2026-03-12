use crate::registry::*;
use alloc::{collections::BTreeMap, vec::Vec};
use bytes::{Buf, Bytes};
use core::{convert::TryInto, str};
use serde::ser::{SerializeMap, SerializeSeq, SerializeTuple, SerializeTupleStruct};
use serde::Serialize;

/// A container for SCALE encoded data that can serialize types directly
/// with the help of a type registry and without using an intermediate representation.
pub struct Value<'a> {
    data: Bytes,
    ty_id: TypeId,
    registry: &'a Registry,
}

impl<'a> Value<'a> {
    pub fn new(data: impl Into<Bytes>, ty_id: TypeId, registry: &'a Registry) -> Self {
        Value {
            data: data.into(),
            ty_id,
            registry,
        }
    }

    fn new_value(&self, data: &mut Bytes, ty_id: TypeId) -> Self {
        let size = self.ty_size(data.chunk(), ty_id);
        Value::new(data.copy_to_bytes(size), ty_id, self.registry)
    }

    #[inline]
    fn resolve(&self, ty: TypeId) -> &'a TypeDef {
        self.registry.resolve(ty).expect("in registry")
    }

    pub fn size(&self) -> usize {
        self.ty_size(&self.data, self.ty_id)
    }

    fn ty_size(&self, data: &[u8], ty: TypeId) -> usize {
        match self.resolve(ty) {
            TypeDef::U8 | TypeDef::I8 | TypeDef::Bool => 1,
            TypeDef::U16 | TypeDef::I16 => 2,
            TypeDef::U32 | TypeDef::I32 | TypeDef::Char => 4,
            TypeDef::U64 | TypeDef::I64 => 8,
            TypeDef::U128 | TypeDef::I128 => 16,
            TypeDef::Str => {
                let (l, p_size) = sequence_size(data);
                l + p_size
            }
            TypeDef::Bytes => {
                let (l, p_size) = sequence_size(data);
                l + p_size
            }
            TypeDef::Struct(fields) => fields
                .iter()
                .fold(0, |c, f| c + self.ty_size(&data[c..], f.ty)),
            TypeDef::StructUnit => 0,
            TypeDef::StructNewType(ty) => self.ty_size(data, *ty),
            TypeDef::StructTuple(fields) => fields
                .iter()
                .fold(0, |c, ty| c + self.ty_size(&data[c..], *ty)),
            TypeDef::Variant(vdef) => {
                let var = vdef
                    .variants
                    .iter()
                    .find(|v| v.index == data[0])
                    .expect("variant");
                match &var.fields {
                    Fields::Unit => 1,
                    Fields::NewType(ty) => 1 + self.ty_size(&data[1..], *ty),
                    Fields::Tuple(tys) => tys
                        .iter()
                        .fold(1, |c, ty| c + self.ty_size(&data[c..], *ty)),
                    Fields::Struct(fields) => fields
                        .iter()
                        .fold(1, |c, f| c + self.ty_size(&data[c..], f.ty)),
                }
            }
            TypeDef::Sequence(ty_id) => {
                let (len, prefix_size) = sequence_size(data);
                (0..len).fold(prefix_size, |c, _| c + self.ty_size(&data[c..], *ty_id))
            }
            TypeDef::Array(ty_id, len) => {
                let element_size = self.ty_size(data, *ty_id);
                element_size * (*len as usize)
            }
            TypeDef::Tuple(fields) => fields
                .iter()
                .fold(0, |c, ty| c + self.ty_size(&data[c..], *ty)),
            TypeDef::Map(ty_k, ty_v) => {
                let (len, prefix_size) = sequence_size(data);
                (0..len).fold(prefix_size, |c, _| {
                    let k = self.ty_size(&data[c..], *ty_k);
                    let v = self.ty_size(&data[c + k..], *ty_v);
                    c + k + v
                })
            }
            TypeDef::Compact(_) => compact_size(data),
            TypeDef::BitSequence(_, _) => {
                // BitVec is encoded as a compact length (in bits) followed by the bytes
                let (bit_len, prefix_size) = sequence_size(data);
                prefix_size + bit_len.div_ceil(8)
            }
        }
    }

    /// Extract a `u8` value if this Value represents a u8
    pub fn as_u8(&self) -> Option<u8> {
        match self.resolve(self.ty_id) {
            TypeDef::U8 if !self.data.is_empty() => Some(self.data[0]),
            _ => None,
        }
    }

    /// Extract a `u16` value if this Value represents a u16
    pub fn as_u16(&self) -> Option<u16> {
        match self.resolve(self.ty_id) {
            TypeDef::U16 if self.data.len() >= 2 => {
                Some(u16::from_le_bytes([self.data[0], self.data[1]]))
            }
            _ => None,
        }
    }

    /// Extract a `u32` value if this Value represents a u32
    pub fn as_u32(&self) -> Option<u32> {
        match self.resolve(self.ty_id) {
            TypeDef::U32 if self.data.len() >= 4 => {
                let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                Some(u32::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract a `u64` value if this Value represents a u64
    pub fn as_u64(&self) -> Option<u64> {
        match self.resolve(self.ty_id) {
            TypeDef::U64 if self.data.len() >= 8 => {
                let bytes: [u8; 8] = self.data[0..8].try_into().ok()?;
                Some(u64::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract a `u128` value if this Value represents a u128
    pub fn as_u128(&self) -> Option<u128> {
        match self.resolve(self.ty_id) {
            TypeDef::U128 if self.data.len() >= 16 => {
                let bytes: [u8; 16] = self.data[0..16].try_into().ok()?;
                Some(u128::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract an `i8` value if this Value represents an i8
    pub fn as_i8(&self) -> Option<i8> {
        match self.resolve(self.ty_id) {
            TypeDef::I8 if !self.data.is_empty() => Some(self.data[0] as i8),
            _ => None,
        }
    }

    /// Extract an `i16` value if this Value represents an i16
    pub fn as_i16(&self) -> Option<i16> {
        match self.resolve(self.ty_id) {
            TypeDef::I16 if self.data.len() >= 2 => {
                Some(i16::from_le_bytes([self.data[0], self.data[1]]))
            }
            _ => None,
        }
    }

    /// Extract an `i32` value if this Value represents an i32
    pub fn as_i32(&self) -> Option<i32> {
        match self.resolve(self.ty_id) {
            TypeDef::I32 if self.data.len() >= 4 => {
                let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                Some(i32::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract an `i64` value if this Value represents an i64
    pub fn as_i64(&self) -> Option<i64> {
        match self.resolve(self.ty_id) {
            TypeDef::I64 if self.data.len() >= 8 => {
                let bytes: [u8; 8] = self.data[0..8].try_into().ok()?;
                Some(i64::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract an `i128` value if this Value represents an i128
    pub fn as_i128(&self) -> Option<i128> {
        match self.resolve(self.ty_id) {
            TypeDef::I128 if self.data.len() >= 16 => {
                let bytes: [u8; 16] = self.data[0..16].try_into().ok()?;
                Some(i128::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract a `bool` value if this Value represents a bool
    pub fn as_bool(&self) -> Option<bool> {
        match self.resolve(self.ty_id) {
            TypeDef::Bool if !self.data.is_empty() => Some(self.data[0] != 0),
            _ => None,
        }
    }

    /// Extract a `char` value if this Value represents a char
    pub fn as_char(&self) -> Option<char> {
        match self.resolve(self.ty_id) {
            TypeDef::Char if self.data.len() >= 4 => {
                let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                char::from_u32(u32::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract a `&str` value if this Value represents a string
    pub fn as_str(&self) -> Option<&str> {
        match self.resolve(self.ty_id) {
            TypeDef::Str => {
                let (len, prefix_size) = sequence_size(&self.data);
                if self.data.len() >= prefix_size + len {
                    str::from_utf8(&self.data[prefix_size..prefix_size + len]).ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Check if this Value represents a given type
    pub fn is_u8(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::U8)
    }
    pub fn is_u16(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::U16)
    }
    pub fn is_u32(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::U32)
    }
    pub fn is_u64(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::U64)
    }
    pub fn is_u128(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::U128)
    }
    pub fn is_i8(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::I8)
    }
    pub fn is_i16(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::I16)
    }
    pub fn is_i32(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::I32)
    }
    pub fn is_i64(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::I64)
    }
    pub fn is_i128(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::I128)
    }
    pub fn is_bool(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::Bool)
    }
    pub fn is_char(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::Char)
    }
    pub fn is_string(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::Str)
    }
    pub fn is_sequence(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::Sequence(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::Array(_, _))
    }
    pub fn is_tuple(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::Tuple(_))
    }
    pub fn is_composite(&self) -> bool {
        matches!(
            self.resolve(self.ty_id),
            TypeDef::Struct(_)
                | TypeDef::StructUnit
                | TypeDef::StructNewType(_)
                | TypeDef::StructTuple(_)
        )
    }
    pub fn is_variant(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::Variant(_))
    }
    pub fn is_compact(&self) -> bool {
        matches!(self.resolve(self.ty_id), TypeDef::Compact(_))
    }

    /// Get the length of a sequence if this Value represents a sequence
    pub fn sequence_len(&self) -> Option<usize> {
        match self.resolve(self.ty_id) {
            TypeDef::Sequence(_) => {
                let (len, _) = sequence_size(&self.data);
                Some(len)
            }
            _ => None,
        }
    }

    /// Get an element from a sequence by index
    pub fn sequence_get(&self, index: usize) -> Option<Value<'a>> {
        match self.resolve(self.ty_id) {
            TypeDef::Sequence(inner_ty) => {
                let (len, prefix_size) = sequence_size(&self.data);
                if index >= len {
                    return None;
                }
                let ty_id = *inner_ty;
                let mut data = self.data.slice(prefix_size..);
                for _ in 0..index {
                    let size = self.ty_size(data.chunk(), ty_id);
                    data.advance(size);
                }
                Some(self.new_value(&mut data, ty_id))
            }
            _ => None,
        }
    }

    /// Get the length of an array if this Value represents an array
    pub fn array_len(&self) -> Option<u32> {
        match self.resolve(self.ty_id) {
            TypeDef::Array(_, len) => Some(*len),
            _ => None,
        }
    }

    /// Get an element from an array by index
    pub fn array_get(&self, index: u32) -> Option<Value<'a>> {
        match self.resolve(self.ty_id) {
            TypeDef::Array(ty_id, len) => {
                if index >= *len {
                    return None;
                }
                let ty_id = *ty_id;
                let element_size = self.ty_size(&self.data, ty_id);
                let start = (index as usize) * element_size;
                let end = start + element_size;
                if end <= self.data.len() {
                    Some(Value::new(
                        self.data.slice(start..end),
                        ty_id,
                        self.registry,
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get a field from a composite type by index
    pub fn field_at(&self, index: usize) -> Option<Value<'a>> {
        match self.resolve(self.ty_id) {
            TypeDef::Struct(fields) => {
                if index >= fields.len() {
                    return None;
                }
                let data = self.data.clone();
                let mut offset = 0;
                for f in fields.iter().take(index) {
                    offset += self.ty_size(&data[offset..], f.ty);
                }
                let field_ty = fields[index].ty;
                let field_size = self.ty_size(&data[offset..], field_ty);
                Some(Value::new(
                    data.slice(offset..offset + field_size),
                    field_ty,
                    self.registry,
                ))
            }
            _ => None,
        }
    }

    /// Get a field from a composite type by name
    pub fn field(&self, name: &str) -> Option<Value<'a>> {
        match self.resolve(self.ty_id) {
            TypeDef::Struct(fields) => {
                let index = fields.iter().position(|f| f.name == name)?;
                self.field_at(index)
            }
            _ => None,
        }
    }

    /// Get the number of fields in a composite type
    pub fn field_count(&self) -> Option<usize> {
        match self.resolve(self.ty_id) {
            TypeDef::Struct(fields) => Some(fields.len()),
            _ => None,
        }
    }

    /// Get an element from a tuple by index
    pub fn tuple_get(&self, index: usize) -> Option<Value<'a>> {
        match self.resolve(self.ty_id) {
            TypeDef::Tuple(fields) => {
                if index >= fields.len() {
                    return None;
                }
                let data = self.data.clone();
                let mut offset = 0;
                for ty in fields.iter().take(index) {
                    offset += self.ty_size(&data[offset..], *ty);
                }
                let ty_id = fields[index];
                let size = self.ty_size(&data[offset..], ty_id);
                Some(Value::new(
                    data.slice(offset..offset + size),
                    ty_id,
                    self.registry,
                ))
            }
            _ => None,
        }
    }

    /// Get the number of elements in a tuple
    pub fn tuple_len(&self) -> Option<usize> {
        match self.resolve(self.ty_id) {
            TypeDef::Tuple(fields) => Some(fields.len()),
            _ => None,
        }
    }

    /// Get the variant index if this Value represents a variant (enum)
    pub fn variant_index(&self) -> Option<u8> {
        match self.resolve(self.ty_id) {
            TypeDef::Variant(_) if !self.data.is_empty() => Some(self.data[0]),
            _ => None,
        }
    }

    /// Get the variant name if this Value represents a variant (enum)
    pub fn variant_name(&self) -> Option<&str> {
        match self.resolve(self.ty_id) {
            TypeDef::Variant(vdef) if !self.data.is_empty() => {
                let idx = self.data[0];
                vdef.variants
                    .iter()
                    .find(|v| v.index == idx)
                    .map(|v| v.name.as_str())
            }
            _ => None,
        }
    }

    /// Get the variant data if this Value represents a variant (enum) with data
    pub fn variant_data(&self) -> Option<Value<'a>> {
        match self.resolve(self.ty_id) {
            TypeDef::Variant(vdef) if !self.data.is_empty() => {
                let idx = self.data[0];
                let var = vdef.variants.iter().find(|v| v.index == idx)?;
                match &var.fields {
                    Fields::Unit => None,
                    Fields::NewType(ty_id) => {
                        Some(Value::new(self.data.slice(1..), *ty_id, self.registry))
                    }
                    _ => None, // Multi-field variants not yet supported
                }
            }
            _ => None,
        }
    }
}

impl Serialize for Value<'_> {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut data = self.data.clone();
        let ty = self.resolve(self.ty_id);

        match ty {
            TypeDef::Bool => ser.serialize_bool(data.get_u8() != 0),
            TypeDef::U8 => ser.serialize_u8(data.get_u8()),
            TypeDef::U16 => ser.serialize_u16(data.get_u16_le()),
            TypeDef::U32 => ser.serialize_u32(data.get_u32_le()),
            TypeDef::U64 => ser.serialize_u64(data.get_u64_le()),
            TypeDef::U128 => ser.serialize_u128(data.get_u128_le()),
            TypeDef::I8 => ser.serialize_i8(data.get_i8()),
            TypeDef::I16 => ser.serialize_i16(data.get_i16_le()),
            TypeDef::I32 => ser.serialize_i32(data.get_i32_le()),
            TypeDef::I64 => ser.serialize_i64(data.get_i64_le()),
            TypeDef::I128 => ser.serialize_i128(data.get_i128_le()),
            TypeDef::Compact(inner_ty) => {
                let v: u128 = match self.resolve(*inner_ty) {
                    TypeDef::U32 => data.get_u32_le() as u128,
                    TypeDef::U64 => data.get_u64_le() as u128,
                    TypeDef::U128 => data.get_u128_le(),
                    _ => unimplemented!(),
                };
                let mut buf = Vec::new();
                crate::compact_encode(v, &mut buf);
                ser.serialize_bytes(&buf)
            }
            TypeDef::Bytes => {
                let (_, s) = sequence_size(data.chunk());
                data.advance(s);
                ser.serialize_bytes(data.chunk())
            }
            TypeDef::Char => ser.serialize_char(char::from_u32(data.get_u32_le()).unwrap()),
            TypeDef::Str => {
                let (_, s) = sequence_size(data.chunk());
                data.advance(s);
                ser.serialize_str(str::from_utf8(data.chunk()).unwrap())
            }
            TypeDef::Sequence(ty_id) => {
                let (len, p_size) = sequence_size(data.chunk());
                data.advance(p_size);
                let mut seq = ser.serialize_seq(Some(len))?;
                for _ in 0..len {
                    seq.serialize_element(&self.new_value(&mut data, *ty_id))?;
                }
                seq.end()
            }
            TypeDef::Map(ty_k, ty_v) => {
                let (len, p_size) = sequence_size(data.chunk());
                data.advance(p_size);
                let mut state = ser.serialize_map(Some(len))?;
                for _ in 0..len {
                    let key = self.new_value(&mut data, *ty_k);
                    let val = self.new_value(&mut data, *ty_v);
                    state.serialize_entry(&key, &val)?;
                }
                state.end()
            }
            TypeDef::Array(ty_id, len) => {
                let mut state = ser.serialize_tuple(*len as usize)?;
                for _ in 0..*len {
                    state.serialize_element(&self.new_value(&mut data, *ty_id))?;
                }
                state.end()
            }
            TypeDef::Tuple(fields) => {
                let mut state = ser.serialize_tuple(fields.len())?;
                for ty_id in fields {
                    state.serialize_element(&self.new_value(&mut data, *ty_id))?;
                }
                state.end()
            }
            TypeDef::Struct(fields) => {
                let mut state = ser.serialize_map(Some(fields.len()))?;
                for f in fields {
                    state.serialize_key(&f.name)?;
                    state.serialize_value(&self.new_value(&mut data, f.ty))?;
                }
                state.end()
            }
            TypeDef::StructUnit => ser.serialize_unit(),
            TypeDef::StructNewType(ty_id) => {
                ser.serialize_newtype_struct("", &self.new_value(&mut data, *ty_id))
            }
            TypeDef::StructTuple(fields) => {
                let mut state = ser.serialize_tuple_struct("", fields.len())?;
                for ty_id in fields {
                    state.serialize_field(&self.new_value(&mut data, *ty_id))?;
                }
                state.end()
            }
            TypeDef::Variant(vdef) => {
                let idx = data.get_u8();
                let var = vdef
                    .variants
                    .iter()
                    .find(|v| v.index == idx)
                    .expect("variant");

                let is_option = vdef.name == "Option";

                match &var.fields {
                    Fields::Unit => {
                        if is_option && var.name == "None" {
                            ser.serialize_none()
                        } else {
                            ser.serialize_str(&var.name)
                        }
                    }
                    Fields::NewType(ty_id) => {
                        if is_option && var.name == "Some" {
                            ser.serialize_some(&self.new_value(&mut data, *ty_id))
                        } else {
                            let mut s = ser.serialize_map(Some(1))?;
                            s.serialize_key(&var.name)?;
                            s.serialize_value(&self.new_value(&mut data, *ty_id))?;
                            s.end()
                        }
                    }
                    Fields::Tuple(tys) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(&var.name)?;
                        s.serialize_value(
                            &tys.iter()
                                .map(|ty| self.new_value(&mut data, *ty))
                                .collect::<Vec<_>>(),
                        )?;
                        s.end()
                    }
                    Fields::Struct(fields) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(&var.name)?;
                        s.serialize_value(&fields.iter().fold(
                            BTreeMap::<&str, Value>::new(),
                            |mut m, f| {
                                m.insert(&f.name, self.new_value(&mut data, f.ty));
                                m
                            },
                        ))?;
                        s.end()
                    }
                }
            }
            TypeDef::BitSequence(_, _) => {
                let (bit_len, prefix_size) = sequence_size(data.chunk());
                data.advance(prefix_size);
                let byte_len = bit_len.div_ceil(8);
                ser.serialize_bytes(&data.chunk()[..byte_len])
            }
        }
    }
}

#[inline]
fn compact_size(data: &[u8]) -> usize {
    match data[0] % 0b100 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => todo!(),
    }
}

fn sequence_size(data: &[u8]) -> (usize, usize) {
    // first byte(s) gives us a hint of the (compact encoded) length
    // https://substrate.dev/docs/en/knowledgebase/advanced/codec#compactgeneral-integers
    let len = compact_size(data);
    (
        match len {
            1 => (data[0] >> 2).into(),
            2 => u16::from((data[0] >> 2) | (data[1] << 6)).into(),
            4 => {
                (((data[0] as u32) >> 2)
                    | ((data[1] as u32) << 6)
                    | ((data[2] as u32) << 14)
                    | ((data[3] as u32) << 22)) as usize
            }
            _ => todo!(),
        },
        len,
    )
}

impl AsRef<[u8]> for Value<'_> {
    fn as_ref(&self) -> &[u8] {
        self.data.as_ref()
    }
}

impl core::fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "Value {{ data: {:?}, type({}): {:?} }}",
            self.data,
            self.ty_id,
            self.resolve(self.ty_id)
        )
    }
}

#[cfg(feature = "json")]
impl core::fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let json = serde_json::to_string(self).map_err(|_| core::fmt::Error)?;
        write!(f, "{}", json)
    }
}

#[cfg(feature = "json")]
impl<'reg> From<Value<'reg>> for serde_json::Value {
    fn from(val: Value<'reg>) -> Self {
        serde_json::value::to_value(val).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use super::*;
    use anyhow::Error;
    use codec::Encode;
    use scale_info::{
        meta_type,
        prelude::{string::String, vec::Vec},
        Registry as SiRegistry, TypeInfo,
    };
    use serde_json::to_value;

    #[test]
    fn test_compact_two_bytes() {
        let data: [u8; 2] = [0x99, 0x01];
        assert_eq!(sequence_size(&data), (102, 2));

        let data: [u8; 2] = [0x15, 0x01];
        assert_eq!(sequence_size(&data), (69, 2));

        let data: [u8; 4] = [0xfe, 0xff, 0x03, 0x00];
        assert_eq!(sequence_size(&data), (65535, 4));
    }

    fn register<T>(_ty: &T) -> (u32, Registry)
    where
        T: TypeInfo + 'static,
    {
        let mut reg = SiRegistry::new();
        let sym = reg.register_type(&meta_type::<T>());
        let portable: scale_info::PortableRegistry = reg.into();
        (sym.id, crate::compress::compress(&portable))
    }

    #[cfg(feature = "json")]
    #[test]
    fn display_as_json() {
        #[derive(Encode, TypeInfo)]
        struct Foo {
            bar: String,
        }
        let in_value = Foo { bar: "BAZ".into() };

        let data = in_value.encode();
        let (id, reg) = register(&in_value);
        let out_value = Value::new(data, id, &reg).to_string();

        assert_eq!("{\"bar\":\"BAZ\"}", out_value);
    }

    #[test]
    fn serialize_u8() -> Result<(), Error> {
        let in_value = u8::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16() -> Result<(), Error> {
        let in_value = u16::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32() -> Result<(), Error> {
        let in_value = u32::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u64() -> Result<(), Error> {
        let in_value = u64::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_bool() -> Result<(), Error> {
        let in_value = true;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i16() -> Result<(), Error> {
        let in_value = i16::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i32() -> Result<(), Error> {
        let in_value = i32::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i64() -> Result<(), Error> {
        let in_value = i64::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple() -> Result<(), Error> {
        let in_value = (u8::MAX, i8::MIN, true);
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple_struct() -> Result<(), Error> {
        #[derive(Encode, TypeInfo, serde::Serialize)]
        struct Baz(String, u16);

        let in_value = Baz("lol".into(), u16::MAX);
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u8array() -> Result<(), Error> {
        let in_value: Vec<u8> = vec![0, 1, 2, 3, 4, 5];
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(&out_value)?, to_value(in_value.as_slice())?);
        Ok(())
    }

    #[test]
    fn serialize_u16array() -> Result<(), Error> {
        let in_value: Vec<u16> = vec![0, 1, 2, 3, 4, 5];
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32array() -> Result<(), Error> {
        let in_value: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u8struct() -> Result<(), Error> {
        #[derive(Encode, TypeInfo, serde::Serialize)]
        struct Bar(u8);

        let in_value = Bar(0xFF);
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u32struct() -> Result<(), Error> {
        #[derive(Encode, TypeInfo, serde::Serialize)]
        struct Bar(u32);

        let in_value = Bar(0xFF_EE_DD_CC);
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u64struct() -> Result<(), Error> {
        #[derive(Encode, TypeInfo, serde::Serialize)]
        struct Bar(u64);

        let in_value = Bar(0xFFEE_DDCC_BBAA_9988);
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_complex_struct_with_enum() -> Result<(), Error> {
        #[derive(Encode, TypeInfo, serde::Serialize)]
        struct Foo {
            a: Bar,
            b: Option<Baz>,
        }
        #[derive(Encode, TypeInfo, serde::Serialize)]
        struct Bar(u8);
        #[derive(Encode, TypeInfo, serde::Serialize)]
        struct Baz(String, u16);

        let in_value = Foo {
            a: Bar(0xFF),
            b: Some(Baz("lol".into(), u16::MAX)),
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_map() -> Result<(), Error> {
        let mut in_value = BTreeMap::<String, u32>::new();
        in_value.insert("key".into(), 1234);
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn test_primitive_extraction() {
        let (id, reg) = register(&42u32);
        let data = 42u32.encode();
        let value = Value::new(data, id, &reg);

        assert!(value.is_u32());
        assert_eq!(value.as_u32(), Some(42));
        assert_eq!(value.as_u64(), None);
    }

    #[test]
    fn test_string_extraction() {
        let (id, reg) = register(&"hello");
        let data = "hello".encode();
        let value = Value::new(data, id, &reg);

        assert!(value.is_string());
        assert_eq!(value.as_str(), Some("hello"));
    }

    #[test]
    fn test_composite_field_access() {
        #[derive(Encode, TypeInfo)]
        struct Foo {
            bar: u32,
            baz: String,
        }

        let foo = Foo {
            bar: 42,
            baz: "hello".into(),
        };
        let data = foo.encode();
        let (id, reg) = register(&foo);
        let value = Value::new(data, id, &reg);

        assert!(value.is_composite());
        assert_eq!(value.field_count(), Some(2));

        let bar = value.field("bar").unwrap();
        assert_eq!(bar.as_u32(), Some(42));

        let baz = value.field("baz").unwrap();
        assert_eq!(baz.as_str(), Some("hello"));

        let bar_by_index = value.field_at(0).unwrap();
        assert_eq!(bar_by_index.as_u32(), Some(42));
    }

    #[test]
    fn test_sequence_operations() {
        let input: Vec<u32> = vec![1, 2, 3, 4, 5];
        let data = input.encode();
        let (id, reg) = register(&input);
        let value = Value::new(data, id, &reg);

        assert!(value.is_sequence());
        assert_eq!(value.sequence_len(), Some(5));

        let first = value.sequence_get(0).unwrap();
        assert_eq!(first.as_u32(), Some(1));

        let third = value.sequence_get(2).unwrap();
        assert_eq!(third.as_u32(), Some(3));

        assert!(value.sequence_get(5).is_none());
    }

    #[test]
    fn test_tuple_operations() {
        let input: (u32, bool, u8) = (42, true, 7);
        let data = input.encode();
        let (id, reg) = register(&input);
        let value = Value::new(data, id, &reg);

        assert!(value.is_tuple());
        assert_eq!(value.tuple_len(), Some(3));

        let first = value.tuple_get(0).unwrap();
        assert_eq!(first.as_u32(), Some(42));

        let second = value.tuple_get(1).unwrap();
        assert_eq!(second.as_bool(), Some(true));

        let third = value.tuple_get(2).unwrap();
        assert_eq!(third.as_u8(), Some(7));

        assert!(value.tuple_get(3).is_none());
    }

    #[test]
    fn test_variant_operations() {
        #[derive(Encode, TypeInfo)]
        enum MyEnum {
            UnitVariant,
            #[allow(dead_code)]
            DataVariant(u32),
        }

        let input = MyEnum::UnitVariant;
        let data = input.encode();
        let (id, reg) = register(&input);
        let value = Value::new(data, id, &reg);

        assert!(value.is_variant());
        assert_eq!(value.variant_index(), Some(0));
        assert_eq!(value.variant_name(), Some("UnitVariant"));
        assert!(value.variant_data().is_none());

        let input = MyEnum::DataVariant(42);
        let data = input.encode();
        let value = Value::new(data, id, &reg);

        assert_eq!(value.variant_index(), Some(1));
        assert_eq!(value.variant_name(), Some("DataVariant"));
        let inner = value.variant_data().unwrap();
        assert_eq!(inner.as_u32(), Some(42));
    }

    #[test]
    fn test_all_unsigned_extraction() {
        let (id, reg) = register(&0u8);
        let v = Value::new(255u8.encode(), id, &reg);
        assert!(v.is_u8());
        assert_eq!(v.as_u8(), Some(255));

        let (id, reg) = register(&0u16);
        let v = Value::new(0xABCDu16.encode(), id, &reg);
        assert!(v.is_u16());
        assert_eq!(v.as_u16(), Some(0xABCD));

        let (id, reg) = register(&0u64);
        let v = Value::new(u64::MAX.encode(), id, &reg);
        assert!(v.is_u64());
        assert_eq!(v.as_u64(), Some(u64::MAX));

        let (id, reg) = register(&0u128);
        let v = Value::new(u128::MAX.encode(), id, &reg);
        assert!(v.is_u128());
        assert_eq!(v.as_u128(), Some(u128::MAX));
    }

    #[test]
    fn test_all_signed_extraction() {
        let (id, reg) = register(&0i8);
        let v = Value::new((-42i8).encode(), id, &reg);
        assert!(v.is_i8());
        assert_eq!(v.as_i8(), Some(-42));

        let (id, reg) = register(&0i16);
        let v = Value::new(i16::MIN.encode(), id, &reg);
        assert!(v.is_i16());
        assert_eq!(v.as_i16(), Some(i16::MIN));

        let (id, reg) = register(&0i32);
        let v = Value::new(i32::MIN.encode(), id, &reg);
        assert!(v.is_i32());
        assert_eq!(v.as_i32(), Some(i32::MIN));

        let (id, reg) = register(&0i64);
        let v = Value::new(i64::MIN.encode(), id, &reg);
        assert!(v.is_i64());
        assert_eq!(v.as_i64(), Some(i64::MIN));

        let (id, reg) = register(&0i128);
        let v = Value::new(i128::MIN.encode(), id, &reg);
        assert!(v.is_i128());
        assert_eq!(v.as_i128(), Some(i128::MIN));
    }

    #[test]
    fn test_bool_extraction() {
        let (id, reg) = register(&true);
        assert!(Value::new(true.encode(), id, &reg).is_bool());
        assert_eq!(Value::new(true.encode(), id, &reg).as_bool(), Some(true));
        assert_eq!(Value::new(false.encode(), id, &reg).as_bool(), Some(false));
    }

    #[test]
    fn test_type_mismatch_returns_none() {
        let (uid, reg) = register(&0u32);
        let v = Value::new(42u32.encode(), uid, &reg);

        assert_eq!(v.as_u8(), None);
        assert_eq!(v.as_u16(), None);
        assert_eq!(v.as_u64(), None);
        assert_eq!(v.as_i32(), None);
        assert_eq!(v.as_str(), None);
        assert_eq!(v.as_bool(), None);
        assert!(!v.is_string());
        assert!(!v.is_bool());
        assert!(!v.is_variant());
        assert!(!v.is_sequence());
        assert!(!v.is_tuple());
        assert!(!v.is_composite());
    }

    #[test]
    fn test_truncated_data_returns_none() {
        let (id, reg) = register(&0u32);
        // only 2 bytes for a u32
        let v = Value::new(vec![0u8, 0], id, &reg);
        assert_eq!(v.as_u32(), None);

        let (id, reg) = register(&0u128);
        let v = Value::new(vec![0u8; 8], id, &reg);
        assert_eq!(v.as_u128(), None);

        // empty data
        let (id, reg) = register(&0u8);
        let v = Value::new(vec![], id, &reg);
        assert_eq!(v.as_u8(), None);
    }

    #[test]
    fn test_field_access_nonexistent() {
        #[derive(Encode, TypeInfo)]
        struct Foo {
            bar: u32,
        }
        let foo = Foo { bar: 1 };
        let (id, reg) = register(&foo);
        let v = Value::new(foo.encode(), id, &reg);

        assert!(v.field("nonexistent").is_none());
        assert!(v.field_at(1).is_none());
        assert!(v.field_at(100).is_none());
    }

    #[test]
    fn test_field_on_non_struct() {
        let (id, reg) = register(&42u32);
        let v = Value::new(42u32.encode(), id, &reg);

        assert!(v.field("x").is_none());
        assert!(v.field_at(0).is_none());
        assert_eq!(v.field_count(), None);
    }

    #[test]
    fn test_sequence_on_non_sequence() {
        let (id, reg) = register(&42u32);
        let v = Value::new(42u32.encode(), id, &reg);

        assert_eq!(v.sequence_len(), None);
        assert!(v.sequence_get(0).is_none());
    }

    #[test]
    fn test_tuple_on_non_tuple() {
        let (id, reg) = register(&42u32);
        let v = Value::new(42u32.encode(), id, &reg);

        assert_eq!(v.tuple_len(), None);
        assert!(v.tuple_get(0).is_none());
    }

    #[test]
    fn test_variant_on_non_variant() {
        let (id, reg) = register(&42u32);
        let v = Value::new(42u32.encode(), id, &reg);

        assert_eq!(v.variant_index(), None);
        assert_eq!(v.variant_name(), None);
        assert!(v.variant_data().is_none());
    }

    #[test]
    fn test_empty_sequence() {
        let input: Vec<u32> = vec![];
        let data = input.encode();
        let (id, reg) = register(&input);
        let v = Value::new(data, id, &reg);

        assert_eq!(v.sequence_len(), Some(0));
        assert!(v.sequence_get(0).is_none());
    }

    #[test]
    fn test_array_operations() {
        let input: [u16; 3] = [10, 20, 30];
        let data = input.encode();
        let (id, reg) = register(&input);
        let v = Value::new(data, id, &reg);

        assert!(v.is_array());
        assert_eq!(v.array_len(), Some(3));
        assert_eq!(v.array_get(0).unwrap().as_u16(), Some(10));
        assert_eq!(v.array_get(2).unwrap().as_u16(), Some(30));
        assert!(v.array_get(3).is_none());
    }

    #[test]
    fn test_compact_value() {
        use codec::Compact;
        let input = Compact(42u32);
        let data = input.encode();
        let (id, reg) = register(&input);
        let v = Value::new(data, id, &reg);

        assert!(v.is_compact());
    }

    #[test]
    fn test_option_none() -> Result<(), Error> {
        let input: Option<u32> = None;
        let data = input.encode();
        let (id, reg) = register(&input);
        let v = Value::new(data, id, &reg);

        assert!(v.is_variant());
        assert_eq!(v.variant_name(), Some("None"));
        assert_eq!(to_value(v)?, serde_json::Value::Null);
        Ok(())
    }

    #[test]
    fn test_option_some() -> Result<(), Error> {
        let input: Option<u32> = Some(42);
        let data = input.encode();
        let (id, reg) = register(&input);
        let v = Value::new(data, id, &reg);

        assert!(v.is_variant());
        assert_eq!(v.variant_name(), Some("Some"));
        assert_eq!(to_value(v)?, to_value(42u32)?);
        Ok(())
    }

    #[test]
    fn test_variant_with_tuple_fields() -> Result<(), Error> {
        #[derive(Encode, TypeInfo, serde::Serialize)]
        enum Msg {
            #[allow(dead_code)]
            A,
            B(u32, String),
        }
        let input = Msg::B(7, "hi".into());
        let data = input.encode();
        let (id, reg) = register(&input);
        let v = Value::new(data, id, &reg);

        assert_eq!(v.variant_name(), Some("B"));
        // tuple variant data is not accessible via variant_data (only NewType)
        assert!(v.variant_data().is_none());
        // but full serialization should still work
        assert_eq!(to_value(v)?, to_value(&input)?);
        Ok(())
    }

    #[test]
    fn test_variant_with_struct_fields() -> Result<(), Error> {
        #[derive(Encode, TypeInfo, serde::Serialize)]
        enum Msg {
            #[allow(dead_code)]
            A,
            B {
                x: u32,
                y: String,
            },
        }
        let input = Msg::B {
            x: 99,
            y: "hello".into(),
        };
        let data = input.encode();
        let (id, reg) = register(&input);
        let v = Value::new(data, id, &reg);

        assert_eq!(v.variant_name(), Some("B"));
        assert_eq!(to_value(v)?, to_value(&input)?);
        Ok(())
    }

    #[test]
    fn test_map_operations() -> Result<(), Error> {
        let mut input = BTreeMap::<String, u32>::new();
        input.insert("a".into(), 1);
        input.insert("b".into(), 2);
        input.insert("c".into(), 3);
        let data = input.encode();
        let (id, reg) = register(&input);
        let v = Value::new(data, id, &reg);

        assert_eq!(to_value(v)?, to_value(&input)?);
        Ok(())
    }

    #[test]
    fn test_empty_struct() {
        #[derive(Encode, TypeInfo)]
        struct Unit;
        let data = Unit.encode();
        let (id, reg) = register(&Unit);
        let v = Value::new(data, id, &reg);

        // StructUnit is not composite (no fields)
        assert_eq!(v.field_count(), None);
    }

    #[test]
    fn test_size_calculation() {
        // primitives
        let (id, reg) = register(&0u8);
        assert_eq!(Value::new(0u8.encode(), id, &reg).size(), 1);

        let (id, reg) = register(&0u32);
        assert_eq!(Value::new(0u32.encode(), id, &reg).size(), 4);

        let (id, reg) = register(&0u128);
        assert_eq!(Value::new(0u128.encode(), id, &reg).size(), 16);

        // string
        let (id, reg) = register(&"hello");
        assert_eq!(Value::new("hello".encode(), id, &reg).size(), 1 + 5); // compact(5) + 5 bytes

        // sequence
        let input: Vec<u32> = vec![1, 2, 3];
        let (id, reg) = register(&input);
        assert_eq!(Value::new(input.encode(), id, &reg).size(), 1 + 3 * 4); // compact(3) + 3*4
    }

    #[test]
    fn test_registry_resolve_invalid_id() {
        let reg = Registry::new(vec![TypeDef::U8]);
        assert!(reg.resolve(0).is_some());
        assert!(reg.resolve(1).is_none());
        assert!(reg.resolve(u32::MAX).is_none());
    }
}
