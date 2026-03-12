use crate::error::Error;
use crate::registry::*;
use bytes::Buf;
use core::{convert::TryInto, str};
use serde::ser::{SerializeMap, SerializeSeq, SerializeTuple, SerializeTupleStruct};
use serde::Serialize;

/// A container for SCALE encoded data that can serialize types directly
/// with the help of a type registry and without using an intermediate representation.
pub struct Value<'a> {
    data: &'a [u8],
    ty_id: TypeId,
    registry: &'a Registry,
}

impl<'a> Value<'a> {
    pub fn new(data: &'a [u8], ty_id: TypeId, registry: &'a Registry) -> Self {
        Value {
            data,
            ty_id,
            registry,
        }
    }

    fn new_value(&self, data: &mut &'a [u8], ty_id: TypeId) -> Result<Self, Error> {
        let size = self.ty_size(data, ty_id)?;
        if data.len() < size {
            return Err(Error::Eof);
        }
        let (chunk, rest) = data.split_at(size);
        *data = rest;
        Ok(Value {
            data: chunk,
            ty_id,
            registry: self.registry,
        })
    }

    #[inline]
    fn resolve(&self, ty: TypeId) -> Result<&'a TypeDef, Error> {
        self.registry.resolve(ty).ok_or(Error::TypeNotFound(ty))
    }

    pub fn size(&self) -> Result<usize, Error> {
        self.ty_size(self.data, self.ty_id)
    }

    fn ty_size(&self, data: &[u8], ty: TypeId) -> Result<usize, Error> {
        match self.resolve(ty)? {
            TypeDef::U8 | TypeDef::I8 | TypeDef::Bool => Ok(1),
            TypeDef::U16 | TypeDef::I16 => Ok(2),
            TypeDef::U32 | TypeDef::I32 | TypeDef::Char => Ok(4),
            TypeDef::U64 | TypeDef::I64 => Ok(8),
            TypeDef::U128 | TypeDef::I128 => Ok(16),
            TypeDef::Str | TypeDef::Bytes => {
                let (l, p) = sequence_size(data)?;
                Ok(l + p)
            }
            TypeDef::Struct(fields) => {
                let mut c = 0usize;
                for f in fields {
                    c += self.ty_size(data.get(c..).ok_or(Error::Eof)?, f.ty)?;
                }
                Ok(c)
            }
            TypeDef::StructUnit => Ok(0),
            TypeDef::StructNewType(ty) => self.ty_size(data, *ty),
            TypeDef::StructTuple(fields) => {
                let mut c = 0usize;
                for ty in fields {
                    c += self.ty_size(data.get(c..).ok_or(Error::Eof)?, *ty)?;
                }
                Ok(c)
            }
            TypeDef::Variant(vdef) => {
                if data.is_empty() {
                    return Err(Error::Eof);
                }
                let var = vdef
                    .variants
                    .iter()
                    .find(|v| v.index == data[0])
                    .ok_or(Error::InvalidVariant(data[0]))?;
                match &var.fields {
                    Fields::Unit => Ok(1),
                    Fields::NewType(ty) => {
                        Ok(1 + self.ty_size(data.get(1..).ok_or(Error::Eof)?, *ty)?)
                    }
                    Fields::Tuple(tys) => {
                        let mut c = 1usize;
                        for ty in tys {
                            c += self.ty_size(data.get(c..).ok_or(Error::Eof)?, *ty)?;
                        }
                        Ok(c)
                    }
                    Fields::Struct(fields) => {
                        let mut c = 1usize;
                        for f in fields {
                            c += self.ty_size(data.get(c..).ok_or(Error::Eof)?, f.ty)?;
                        }
                        Ok(c)
                    }
                }
            }
            TypeDef::Sequence(ty_id) => {
                let (len, prefix_size) = sequence_size(data)?;
                if len > data.len() {
                    return Err(Error::Eof);
                }
                let mut c = prefix_size;
                for _ in 0..len {
                    c += self.ty_size(data.get(c..).ok_or(Error::Eof)?, *ty_id)?;
                }
                Ok(c)
            }
            TypeDef::Array(ty_id, len) => {
                let element_size = self.ty_size(data, *ty_id)?;
                Ok(element_size.checked_mul(*len as usize).ok_or(Error::Eof)?)
            }
            TypeDef::Tuple(fields) => {
                let mut c = 0usize;
                for ty in fields {
                    c += self.ty_size(data.get(c..).ok_or(Error::Eof)?, *ty)?;
                }
                Ok(c)
            }
            TypeDef::Map(ty_k, ty_v) => {
                let (len, prefix_size) = sequence_size(data)?;
                if len > data.len() {
                    return Err(Error::Eof);
                }
                let mut c = prefix_size;
                for _ in 0..len {
                    let d = data.get(c..).ok_or(Error::Eof)?;
                    let k = self.ty_size(d, *ty_k)?;
                    let v = self.ty_size(d.get(k..).ok_or(Error::Eof)?, *ty_v)?;
                    c += k + v;
                }
                Ok(c)
            }
            TypeDef::Compact(_) => compact_size(data),
            TypeDef::BitSequence(_, _) => {
                let (bit_len, prefix_size) = sequence_size(data)?;
                Ok(prefix_size + bit_len.div_ceil(8))
            }
        }
    }

    /// Extract a `u8` value if this Value represents a u8
    pub fn as_u8(&self) -> Option<u8> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::U8 if !self.data.is_empty() => Some(self.data[0]),
            _ => None,
        }
    }

    /// Extract a `u16` value if this Value represents a u16
    pub fn as_u16(&self) -> Option<u16> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::U16 if self.data.len() >= 2 => {
                Some(u16::from_le_bytes([self.data[0], self.data[1]]))
            }
            _ => None,
        }
    }

    /// Extract a `u32` value if this Value represents a u32
    pub fn as_u32(&self) -> Option<u32> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::U32 if self.data.len() >= 4 => {
                let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                Some(u32::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract a `u64` value if this Value represents a u64
    pub fn as_u64(&self) -> Option<u64> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::U64 if self.data.len() >= 8 => {
                let bytes: [u8; 8] = self.data[0..8].try_into().ok()?;
                Some(u64::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract a `u128` value if this Value represents a u128
    pub fn as_u128(&self) -> Option<u128> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::U128 if self.data.len() >= 16 => {
                let bytes: [u8; 16] = self.data[0..16].try_into().ok()?;
                Some(u128::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract an `i8` value if this Value represents an i8
    pub fn as_i8(&self) -> Option<i8> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::I8 if !self.data.is_empty() => Some(self.data[0] as i8),
            _ => None,
        }
    }

    /// Extract an `i16` value if this Value represents an i16
    pub fn as_i16(&self) -> Option<i16> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::I16 if self.data.len() >= 2 => {
                Some(i16::from_le_bytes([self.data[0], self.data[1]]))
            }
            _ => None,
        }
    }

    /// Extract an `i32` value if this Value represents an i32
    pub fn as_i32(&self) -> Option<i32> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::I32 if self.data.len() >= 4 => {
                let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                Some(i32::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract an `i64` value if this Value represents an i64
    pub fn as_i64(&self) -> Option<i64> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::I64 if self.data.len() >= 8 => {
                let bytes: [u8; 8] = self.data[0..8].try_into().ok()?;
                Some(i64::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract an `i128` value if this Value represents an i128
    pub fn as_i128(&self) -> Option<i128> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::I128 if self.data.len() >= 16 => {
                let bytes: [u8; 16] = self.data[0..16].try_into().ok()?;
                Some(i128::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract a `bool` value if this Value represents a bool
    pub fn as_bool(&self) -> Option<bool> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Bool if !self.data.is_empty() => Some(self.data[0] != 0),
            _ => None,
        }
    }

    /// Extract a `char` value if this Value represents a char
    pub fn as_char(&self) -> Option<char> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Char if self.data.len() >= 4 => {
                let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                char::from_u32(u32::from_le_bytes(bytes))
            }
            _ => None,
        }
    }

    /// Extract a `&str` value if this Value represents a string
    pub fn as_str(&self) -> Option<&str> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Str => {
                let (len, prefix_size) = sequence_size(self.data).ok()?;
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
        matches!(self.resolve(self.ty_id), Ok(TypeDef::U8))
    }
    pub fn is_u16(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::U16))
    }
    pub fn is_u32(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::U32))
    }
    pub fn is_u64(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::U64))
    }
    pub fn is_u128(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::U128))
    }
    pub fn is_i8(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::I8))
    }
    pub fn is_i16(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::I16))
    }
    pub fn is_i32(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::I32))
    }
    pub fn is_i64(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::I64))
    }
    pub fn is_i128(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::I128))
    }
    pub fn is_bool(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::Bool))
    }
    pub fn is_char(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::Char))
    }
    pub fn is_string(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::Str))
    }
    pub fn is_sequence(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::Sequence(_)))
    }
    pub fn is_array(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::Array(_, _)))
    }
    pub fn is_tuple(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::Tuple(_)))
    }
    pub fn is_composite(&self) -> bool {
        matches!(
            self.resolve(self.ty_id),
            Ok(TypeDef::Struct(_)
                | TypeDef::StructUnit
                | TypeDef::StructNewType(_)
                | TypeDef::StructTuple(_))
        )
    }
    pub fn is_variant(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::Variant(_)))
    }
    pub fn is_compact(&self) -> bool {
        matches!(self.resolve(self.ty_id), Ok(TypeDef::Compact(_)))
    }

    /// Get the length of a sequence if this Value represents a sequence
    pub fn sequence_len(&self) -> Option<usize> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Sequence(_) => {
                let (len, _) = sequence_size(self.data).ok()?;
                Some(len)
            }
            _ => None,
        }
    }

    /// Get an element from a sequence by index
    pub fn sequence_get(&self, index: usize) -> Option<Value<'a>> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Sequence(inner_ty) => {
                let (len, prefix_size) = sequence_size(self.data).ok()?;
                if index >= len {
                    return None;
                }
                let ty_id = *inner_ty;
                let mut data = &self.data[prefix_size..];
                for _ in 0..index {
                    let size = self.ty_size(data, ty_id).ok()?;
                    data = &data[size..];
                }
                self.new_value(&mut data, ty_id).ok()
            }
            _ => None,
        }
    }

    /// Get the length of an array if this Value represents an array
    pub fn array_len(&self) -> Option<u32> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Array(_, len) => Some(*len),
            _ => None,
        }
    }

    /// Get an element from an array by index
    pub fn array_get(&self, index: u32) -> Option<Value<'a>> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Array(ty_id, len) => {
                if index >= *len {
                    return None;
                }
                let ty_id = *ty_id;
                let element_size = self.ty_size(self.data, ty_id).ok()?;
                let start = (index as usize).checked_mul(element_size)?;
                let end = start.checked_add(element_size)?;
                if end <= self.data.len() {
                    Some(Value::new(&self.data[start..end], ty_id, self.registry))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get a field from a composite type by index
    pub fn field_at(&self, index: usize) -> Option<Value<'a>> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Struct(fields) => {
                if index >= fields.len() {
                    return None;
                }
                let mut offset = 0;
                for f in fields.iter().take(index) {
                    offset += self.ty_size(self.data.get(offset..)?, f.ty).ok()?;
                }
                let field_ty = fields[index].ty;
                let field_size = self.ty_size(self.data.get(offset..)?, field_ty).ok()?;
                Some(Value::new(
                    &self.data[offset..offset + field_size],
                    field_ty,
                    self.registry,
                ))
            }
            _ => None,
        }
    }

    /// Get a field from a composite type by name
    pub fn field(&self, name: &str) -> Option<Value<'a>> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Struct(fields) => {
                let index = fields.iter().position(|f| f.name == name)?;
                self.field_at(index)
            }
            _ => None,
        }
    }

    /// Get the number of fields in a composite type
    pub fn field_count(&self) -> Option<usize> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Struct(fields) => Some(fields.len()),
            _ => None,
        }
    }

    /// Get an element from a tuple by index
    pub fn tuple_get(&self, index: usize) -> Option<Value<'a>> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Tuple(fields) => {
                if index >= fields.len() {
                    return None;
                }
                let mut offset = 0;
                for ty in fields.iter().take(index) {
                    offset += self.ty_size(self.data.get(offset..)?, *ty).ok()?;
                }
                let ty_id = fields[index];
                let size = self.ty_size(self.data.get(offset..)?, ty_id).ok()?;
                Some(Value::new(
                    &self.data[offset..offset + size],
                    ty_id,
                    self.registry,
                ))
            }
            _ => None,
        }
    }

    /// Get the number of elements in a tuple
    pub fn tuple_len(&self) -> Option<usize> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Tuple(fields) => Some(fields.len()),
            _ => None,
        }
    }

    /// Get the variant index if this Value represents a variant (enum)
    pub fn variant_index(&self) -> Option<u8> {
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Variant(_) if !self.data.is_empty() => Some(self.data[0]),
            _ => None,
        }
    }

    /// Get the variant name if this Value represents a variant (enum)
    pub fn variant_name(&self) -> Option<&str> {
        match self.resolve(self.ty_id).ok()? {
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
        match self.resolve(self.ty_id).ok()? {
            TypeDef::Variant(vdef) if !self.data.is_empty() => {
                let idx = self.data[0];
                let var = vdef.variants.iter().find(|v| v.index == idx)?;
                match &var.fields {
                    Fields::Unit => None,
                    Fields::NewType(ty_id) => {
                        Some(Value::new(&self.data[1..], *ty_id, self.registry))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

use alloc::collections::BTreeMap;
use serde::ser::Error as _;

impl Serialize for Value<'_> {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut data: &[u8] = self.data;
        let ty = self.resolve(self.ty_id).map_err(S::Error::custom)?;

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
                let inner = self.resolve(*inner_ty).map_err(S::Error::custom)?;
                let v: u128 = match inner {
                    TypeDef::U32 => data.get_u32_le() as u128,
                    TypeDef::U64 => data.get_u64_le() as u128,
                    TypeDef::U128 => data.get_u128_le(),
                    _ => {
                        return Err(S::Error::custom(Error::BadType(
                            "unsupported compact inner type".into(),
                        )))
                    }
                };
                let mut buf = [0u8; 17];
                let mut writer: &mut [u8] = &mut buf;
                crate::compact_encode(v, &mut writer);
                let written = 17 - writer.len();
                ser.serialize_bytes(&buf[..written])
            }
            TypeDef::Bytes => {
                let (_, s) = sequence_size(data).map_err(S::Error::custom)?;
                data.advance(s);
                ser.serialize_bytes(data)
            }
            TypeDef::Char => {
                let code = data.get_u32_le();
                let c = char::from_u32(code)
                    .ok_or_else(|| S::Error::custom(Error::InvalidChar(code)))?;
                ser.serialize_char(c)
            }
            TypeDef::Str => {
                let (_, s) = sequence_size(data).map_err(S::Error::custom)?;
                data.advance(s);
                let text =
                    str::from_utf8(data).map_err(|_| S::Error::custom(Error::InvalidUtf8))?;
                ser.serialize_str(text)
            }
            TypeDef::Sequence(ty_id) => {
                let (len, p_size) = sequence_size(data).map_err(S::Error::custom)?;
                data.advance(p_size);
                let mut seq = ser.serialize_seq(Some(len))?;
                for _ in 0..len {
                    let v = self
                        .new_value(&mut data, *ty_id)
                        .map_err(S::Error::custom)?;
                    seq.serialize_element(&v)?;
                }
                seq.end()
            }
            TypeDef::Map(ty_k, ty_v) => {
                let (len, p_size) = sequence_size(data).map_err(S::Error::custom)?;
                data.advance(p_size);
                let mut state = ser.serialize_map(Some(len))?;
                for _ in 0..len {
                    let key = self.new_value(&mut data, *ty_k).map_err(S::Error::custom)?;
                    let val = self.new_value(&mut data, *ty_v).map_err(S::Error::custom)?;
                    state.serialize_entry(&key, &val)?;
                }
                state.end()
            }
            TypeDef::Array(ty_id, len) => {
                let mut state = ser.serialize_tuple(*len as usize)?;
                for _ in 0..*len {
                    let v = self
                        .new_value(&mut data, *ty_id)
                        .map_err(S::Error::custom)?;
                    state.serialize_element(&v)?;
                }
                state.end()
            }
            TypeDef::Tuple(fields) => {
                let mut state = ser.serialize_tuple(fields.len())?;
                for ty_id in fields {
                    let v = self
                        .new_value(&mut data, *ty_id)
                        .map_err(S::Error::custom)?;
                    state.serialize_element(&v)?;
                }
                state.end()
            }
            TypeDef::Struct(fields) => {
                let mut state = ser.serialize_map(Some(fields.len()))?;
                for f in fields {
                    state.serialize_key(&f.name)?;
                    let v = self.new_value(&mut data, f.ty).map_err(S::Error::custom)?;
                    state.serialize_value(&v)?;
                }
                state.end()
            }
            TypeDef::StructUnit => ser.serialize_unit(),
            TypeDef::StructNewType(ty_id) => {
                let v = self
                    .new_value(&mut data, *ty_id)
                    .map_err(S::Error::custom)?;
                ser.serialize_newtype_struct("", &v)
            }
            TypeDef::StructTuple(fields) => {
                let mut state = ser.serialize_tuple_struct("", fields.len())?;
                for ty_id in fields {
                    let v = self
                        .new_value(&mut data, *ty_id)
                        .map_err(S::Error::custom)?;
                    state.serialize_field(&v)?;
                }
                state.end()
            }
            TypeDef::Variant(vdef) => {
                if data.is_empty() {
                    return Err(S::Error::custom(Error::Eof));
                }
                let idx = data.get_u8();
                let var = vdef
                    .variants
                    .iter()
                    .find(|v| v.index == idx)
                    .ok_or_else(|| S::Error::custom(Error::InvalidVariant(idx)))?;

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
                        let v = self
                            .new_value(&mut data, *ty_id)
                            .map_err(S::Error::custom)?;
                        if is_option && var.name == "Some" {
                            ser.serialize_some(&v)
                        } else {
                            let mut s = ser.serialize_map(Some(1))?;
                            s.serialize_key(&var.name)?;
                            s.serialize_value(&v)?;
                            s.end()
                        }
                    }
                    Fields::Tuple(tys) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(&var.name)?;
                        let vals: Result<alloc::vec::Vec<_>, _> = tys
                            .iter()
                            .map(|ty| self.new_value(&mut data, *ty).map_err(S::Error::custom))
                            .collect();
                        s.serialize_value(&vals?)?;
                        s.end()
                    }
                    Fields::Struct(fields) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(&var.name)?;
                        // TODO: avoid BTreeMap allocation with a custom Serialize wrapper
                        let mut m = BTreeMap::<&str, Value>::new();
                        for f in fields {
                            let v = self.new_value(&mut data, f.ty).map_err(S::Error::custom)?;
                            m.insert(&f.name, v);
                        }
                        s.serialize_value(&m)?;
                        s.end()
                    }
                }
            }
            TypeDef::BitSequence(_, _) => {
                let (bit_len, prefix_size) = sequence_size(data).map_err(S::Error::custom)?;
                data.advance(prefix_size);
                let byte_len = bit_len.div_ceil(8);
                if data.len() < byte_len {
                    return Err(S::Error::custom(Error::Eof));
                }
                ser.serialize_bytes(&data[..byte_len])
            }
        }
    }
}

#[inline]
fn compact_size(data: &[u8]) -> Result<usize, Error> {
    if data.is_empty() {
        return Err(Error::Eof);
    }
    match data[0] % 4 {
        0 => Ok(1),
        1 => {
            if data.len() < 2 {
                return Err(Error::Eof);
            }
            Ok(2)
        }
        2 => {
            if data.len() < 4 {
                return Err(Error::Eof);
            }
            Ok(4)
        }
        _ => {
            // big-integer mode: upper 6 bits encode (byte_count - 4)
            let bytes_needed = (data[0] >> 2) as usize + 4;
            let total = 1 + bytes_needed;
            if data.len() < total {
                return Err(Error::Eof);
            }
            Ok(total)
        }
    }
}

pub(crate) fn sequence_size(data: &[u8]) -> Result<(usize, usize), Error> {
    let prefix = compact_size(data)?;
    let len = match prefix {
        1 => (data[0] >> 2) as usize,
        2 => u16::from((data[0] >> 2) | (data[1] << 6)) as usize,
        4 => {
            (((data[0] as u32) >> 2)
                | ((data[1] as u32) << 6)
                | ((data[2] as u32) << 14)
                | ((data[3] as u32) << 22)) as usize
        }
        n => {
            // big-integer mode: read (n-1) bytes as LE integer
            let byte_count = n - 1;
            let mut value: u64 = 0;
            for i in 0..byte_count.min(8) {
                value |= (data[1 + i] as u64) << (i * 8);
            }
            value as usize
        }
    };
    Ok((len, prefix))
}

impl AsRef<[u8]> for Value<'_> {
    fn as_ref(&self) -> &[u8] {
        self.data
    }
}

impl core::fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "Value {{ data: {:?}, type({}): {:?} }}",
            self.data,
            self.ty_id,
            self.registry.resolve(self.ty_id)
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
