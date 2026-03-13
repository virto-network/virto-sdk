use crate::error::Error;
use crate::registry::*;
use bytes::Buf;
use core::str;
use serde::ser::{SerializeMap, SerializeSeq, SerializeTuple, SerializeTupleStruct};
use serde::Serialize;

/// Compute the byte size of a SCALE-encoded value given its type.
pub(crate) fn ty_size(registry: &Registry, data: &[u8], ty: TypeId) -> Result<usize, Error> {
    match registry.resolve(ty).ok_or(Error::TypeNotFound(ty))? {
        TypeDef::U8 | TypeDef::I8 | TypeDef::Bool => Ok(1),
        TypeDef::U16 | TypeDef::I16 => Ok(2),
        TypeDef::U32 | TypeDef::I32 | TypeDef::Char => Ok(4),
        TypeDef::U64 | TypeDef::I64 => Ok(8),
        TypeDef::U128 | TypeDef::I128 => Ok(16),
        TypeDef::Str | TypeDef::Bytes => {
            let (l, p) = sequence_size(data)?;
            Ok(l + p)
        }
        TypeDef::StructUnit => Ok(0),
        TypeDef::StructNewType(inner) => ty_size(registry, data, *inner),
        TypeDef::Struct(fields) => fields.iter().try_fold(0usize, |c, f| {
            Ok(c + ty_size(registry, data.get(c..).ok_or(Error::Eof)?, f.ty)?)
        }),
        TypeDef::StructTuple(tys) | TypeDef::Tuple(tys) => tys.iter().try_fold(0usize, |c, ty| {
            Ok(c + ty_size(registry, data.get(c..).ok_or(Error::Eof)?, *ty)?)
        }),
        TypeDef::Variant(vdef) => {
            let &idx = data.first().ok_or(Error::Eof)?;
            let var = vdef.variant(idx)?;
            match &var.fields {
                Fields::Unit => Ok(1),
                Fields::NewType(ty) => {
                    Ok(1 + ty_size(registry, data.get(1..).ok_or(Error::Eof)?, *ty)?)
                }
                Fields::Tuple(tys) => tys.iter().try_fold(1usize, |c, ty| {
                    Ok(c + ty_size(registry, data.get(c..).ok_or(Error::Eof)?, *ty)?)
                }),
                Fields::Struct(fields) => fields.iter().try_fold(1usize, |c, f| {
                    Ok(c + ty_size(registry, data.get(c..).ok_or(Error::Eof)?, f.ty)?)
                }),
            }
        }
        TypeDef::Sequence(ty_id) => {
            let (len, prefix_size) = sequence_size(data)?;
            (0..len).try_fold(prefix_size, |c, _| {
                Ok(c + ty_size(registry, data.get(c..).ok_or(Error::Eof)?, *ty_id)?)
            })
        }
        TypeDef::Array(ty_id, len) => {
            let element_size = ty_size(registry, data, *ty_id)?;
            element_size.checked_mul(*len as usize).ok_or(Error::Eof)
        }
        TypeDef::Map(ty_k, ty_v) => {
            let (len, prefix_size) = sequence_size(data)?;
            (0..len).try_fold(prefix_size, |c, _| {
                let d = data.get(c..).ok_or(Error::Eof)?;
                let k = ty_size(registry, d, *ty_k)?;
                let v = ty_size(registry, d.get(k..).ok_or(Error::Eof)?, *ty_v)?;
                Ok(c + k + v)
            })
        }
        TypeDef::Compact(_) => compact_size(data),
        TypeDef::BitSequence(_, _) => {
            let (bit_len, prefix_size) = sequence_size(data)?;
            Ok(prefix_size + bit_len.div_ceil(8))
        }
    }
}

/// A container for SCALE encoded data that can serialize types directly
/// with the help of a type registry and without using an intermediate representation.
pub struct Value<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) ty_id: TypeId,
    pub(crate) registry: &'a Registry,
}

impl<'a> Value<'a> {
    pub fn new(data: &'a [u8], ty_id: TypeId, registry: &'a Registry) -> Self {
        Value {
            data,
            ty_id,
            registry,
        }
    }

    /// The underlying type definition.
    #[inline]
    pub fn ty(&self) -> Option<&'a TypeDef> {
        self.registry.resolve(self.ty_id)
    }

    pub fn size(&self) -> Result<usize, Error> {
        ty_size(self.registry, self.data, self.ty_id)
    }

    fn cursor(&self) -> Cursor<'a> {
        Cursor::new(self.data, self.registry)
    }

    // primitive extraction

    fn read_le<const N: usize>(&self) -> Option<[u8; N]> {
        self.data.get(..N)?.try_into().ok()
    }

    pub fn as_u8(&self) -> Option<u8> {
        matches!(self.ty()?, TypeDef::U8).then(|| ())?;
        self.data.first().copied()
    }
    pub fn as_u16(&self) -> Option<u16> {
        matches!(self.ty()?, TypeDef::U16).then(|| ())?;
        Some(u16::from_le_bytes(self.read_le()?))
    }
    pub fn as_u32(&self) -> Option<u32> {
        matches!(self.ty()?, TypeDef::U32).then(|| ())?;
        Some(u32::from_le_bytes(self.read_le()?))
    }
    pub fn as_u64(&self) -> Option<u64> {
        matches!(self.ty()?, TypeDef::U64).then(|| ())?;
        Some(u64::from_le_bytes(self.read_le()?))
    }
    pub fn as_u128(&self) -> Option<u128> {
        matches!(self.ty()?, TypeDef::U128).then(|| ())?;
        Some(u128::from_le_bytes(self.read_le()?))
    }
    pub fn as_i8(&self) -> Option<i8> {
        matches!(self.ty()?, TypeDef::I8).then(|| ())?;
        self.data.first().map(|&b| b as i8)
    }
    pub fn as_i16(&self) -> Option<i16> {
        matches!(self.ty()?, TypeDef::I16).then(|| ())?;
        Some(i16::from_le_bytes(self.read_le()?))
    }
    pub fn as_i32(&self) -> Option<i32> {
        matches!(self.ty()?, TypeDef::I32).then(|| ())?;
        Some(i32::from_le_bytes(self.read_le()?))
    }
    pub fn as_i64(&self) -> Option<i64> {
        matches!(self.ty()?, TypeDef::I64).then(|| ())?;
        Some(i64::from_le_bytes(self.read_le()?))
    }
    pub fn as_i128(&self) -> Option<i128> {
        matches!(self.ty()?, TypeDef::I128).then(|| ())?;
        Some(i128::from_le_bytes(self.read_le()?))
    }
    pub fn as_bool(&self) -> Option<bool> {
        matches!(self.ty()?, TypeDef::Bool).then(|| ())?;
        self.data.first().map(|&b| b != 0)
    }

    pub fn as_char(&self) -> Option<char> {
        if !matches!(self.ty()?, TypeDef::Char) {
            return None;
        }
        char::from_u32(u32::from_le_bytes(self.read_le()?))
    }

    pub fn as_str(&self) -> Option<&'a str> {
        if !matches!(self.ty()?, TypeDef::Str) {
            return None;
        }
        let (len, prefix) = sequence_size(self.data).ok()?;
        str::from_utf8(self.data.get(prefix..prefix + len)?).ok()
    }

    // type predicates

    pub fn is_u8(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::U8))
    }
    pub fn is_u16(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::U16))
    }
    pub fn is_u32(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::U32))
    }
    pub fn is_u64(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::U64))
    }
    pub fn is_u128(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::U128))
    }
    pub fn is_i8(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::I8))
    }
    pub fn is_i16(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::I16))
    }
    pub fn is_i32(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::I32))
    }
    pub fn is_i64(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::I64))
    }
    pub fn is_i128(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::I128))
    }
    pub fn is_bool(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::Bool))
    }
    pub fn is_char(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::Char))
    }
    pub fn is_string(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::Str))
    }
    pub fn is_sequence(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::Sequence(_)))
    }
    pub fn is_array(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::Array(..)))
    }
    pub fn is_tuple(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::Tuple(_)))
    }
    pub fn is_compact(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::Compact(_)))
    }
    pub fn is_variant(&self) -> bool {
        matches!(self.ty(), Some(TypeDef::Variant(_)))
    }
    pub fn is_composite(&self) -> bool {
        matches!(
            self.ty(),
            Some(
                TypeDef::Struct(_)
                    | TypeDef::StructUnit
                    | TypeDef::StructNewType(_)
                    | TypeDef::StructTuple(_)
            )
        )
    }

    // sequence access

    pub fn sequence_len(&self) -> Option<usize> {
        matches!(self.ty()?, TypeDef::Sequence(_))
            .then(|| sequence_size(self.data).ok().map(|(len, _)| len))?
    }

    pub fn sequence_get(&self, index: usize) -> Option<Value<'a>> {
        let TypeDef::Sequence(inner_ty) = self.ty()? else {
            return None;
        };
        let (len, prefix_size) = sequence_size(self.data).ok()?;
        if index >= len {
            return None;
        }
        let ty_id = *inner_ty;
        let mut cursor = Cursor::new(&self.data[prefix_size..], self.registry);
        for _ in 0..index {
            cursor.next_value(ty_id).ok()?;
        }
        cursor.next_value(ty_id).ok()
    }

    pub fn sequence_iter(&self) -> Option<SeqIter<'a>> {
        let TypeDef::Sequence(inner_ty) = self.ty()? else {
            return None;
        };
        let (len, prefix_size) = sequence_size(self.data).ok()?;
        Some(SeqIter {
            cursor: Cursor::new(&self.data[prefix_size..], self.registry),
            ty_id: *inner_ty,
            remaining: len,
        })
    }

    // array access

    pub fn array_len(&self) -> Option<u32> {
        let TypeDef::Array(_, len) = self.ty()? else {
            return None;
        };
        Some(*len)
    }

    pub fn array_get(&self, index: u32) -> Option<Value<'a>> {
        let TypeDef::Array(ty_id, len) = self.ty()? else {
            return None;
        };
        if index >= *len {
            return None;
        }
        let ty_id = *ty_id;
        let element_size = ty_size(self.registry, self.data, ty_id).ok()?;
        let start = (index as usize).checked_mul(element_size)?;
        let end = start.checked_add(element_size)?;
        (end <= self.data.len()).then(|| Value::new(&self.data[start..end], ty_id, self.registry))
    }

    // struct/composite access

    pub fn field_count(&self) -> Option<usize> {
        let TypeDef::Struct(fields) = self.ty()? else {
            return None;
        };
        Some(fields.len())
    }

    pub fn field_at(&self, index: usize) -> Option<Value<'a>> {
        let TypeDef::Struct(fields) = self.ty()? else {
            return None;
        };
        let field = fields.get(index)?;
        let mut offset = 0;
        for f in fields.iter().take(index) {
            offset += ty_size(self.registry, self.data.get(offset..)?, f.ty).ok()?;
        }
        let size = ty_size(self.registry, self.data.get(offset..)?, field.ty).ok()?;
        Some(Value::new(
            &self.data[offset..offset + size],
            field.ty,
            self.registry,
        ))
    }

    pub fn field(&self, name: &str) -> Option<Value<'a>> {
        let TypeDef::Struct(fields) = self.ty()? else {
            return None;
        };
        let mut offset = 0;
        for f in fields {
            let size = ty_size(self.registry, self.data.get(offset..)?, f.ty).ok()?;
            if f.name == name {
                return Some(Value::new(
                    &self.data[offset..offset + size],
                    f.ty,
                    self.registry,
                ));
            }
            offset += size;
        }
        None
    }

    pub fn fields_iter(&self) -> Option<FieldIter<'a>> {
        let TypeDef::Struct(fields) = self.ty()? else {
            return None;
        };
        Some(FieldIter {
            cursor: self.cursor(),
            fields: fields.as_slice(),
            index: 0,
        })
    }

    // tuple access

    pub fn tuple_len(&self) -> Option<usize> {
        let TypeDef::Tuple(tys) = self.ty()? else {
            return None;
        };
        Some(tys.len())
    }

    pub fn tuple_get(&self, index: usize) -> Option<Value<'a>> {
        let TypeDef::Tuple(tys) = self.ty()? else {
            return None;
        };
        let ty_id = *tys.get(index)?;
        let mut offset = 0;
        for ty in tys.iter().take(index) {
            offset += ty_size(self.registry, self.data.get(offset..)?, *ty).ok()?;
        }
        let size = ty_size(self.registry, self.data.get(offset..)?, ty_id).ok()?;
        Some(Value::new(
            &self.data[offset..offset + size],
            ty_id,
            self.registry,
        ))
    }

    pub fn tuple_iter(&self) -> Option<TupleIter<'a>> {
        let TypeDef::Tuple(tys) = self.ty()? else {
            return None;
        };
        Some(TupleIter {
            cursor: self.cursor(),
            types: tys.as_slice(),
            index: 0,
        })
    }

    // variant/enum access

    pub fn variant_index(&self) -> Option<u8> {
        let TypeDef::Variant(_) = self.ty()? else {
            return None;
        };
        self.data.first().copied()
    }

    pub fn variant_name(&self) -> Option<&str> {
        let TypeDef::Variant(vdef) = self.ty()? else {
            return None;
        };
        let idx = *self.data.first()?;
        vdef.variant(idx).ok().map(|v| v.name.as_str())
    }

    pub fn variant_data(&self) -> Option<Value<'a>> {
        let TypeDef::Variant(vdef) = self.ty()? else {
            return None;
        };
        let var = vdef.variant(*self.data.first()?).ok()?;
        let Fields::NewType(ty_id) = &var.fields else {
            return None;
        };
        Some(Value::new(&self.data[1..], *ty_id, self.registry))
    }
}

/// A zero-copy cursor over SCALE-encoded bytes that yields [`Value`]s
/// while advancing through the data.
pub struct Cursor<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) registry: &'a Registry,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8], registry: &'a Registry) -> Self {
        Cursor { data, registry }
    }

    /// Consume the next value of the given type, advancing the cursor.
    pub fn next_value(&mut self, ty_id: TypeId) -> Result<Value<'a>, Error> {
        let size = ty_size(self.registry, self.data, ty_id)?;
        let (chunk, rest) = self.data.split_at(size);
        self.data = rest;
        Ok(Value {
            data: chunk,
            ty_id,
            registry: self.registry,
        })
    }

    pub fn remaining(&self) -> &'a [u8] {
        self.data
    }
}

/// Iterator over elements of a SCALE-encoded sequence.
pub struct SeqIter<'a> {
    cursor: Cursor<'a>,
    ty_id: TypeId,
    remaining: usize,
}

impl<'a> Iterator for SeqIter<'a> {
    type Item = Value<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        (self.remaining > 0).then(|| {
            self.remaining -= 1;
            self.cursor.next_value(self.ty_id).ok()
        })?
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for SeqIter<'_> {}

/// Iterator over named fields of a SCALE-encoded struct.
pub struct FieldIter<'a> {
    cursor: Cursor<'a>,
    fields: &'a [Field],
    index: usize,
}

impl<'a> Iterator for FieldIter<'a> {
    type Item = (&'a str, Value<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let field = self.fields.get(self.index)?;
        self.index += 1;
        let val = self.cursor.next_value(field.ty).ok()?;
        Some((&field.name, val))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.fields.len() - self.index;
        (rem, Some(rem))
    }
}

impl ExactSizeIterator for FieldIter<'_> {}

/// Iterator over elements of a SCALE-encoded tuple.
pub struct TupleIter<'a> {
    cursor: Cursor<'a>,
    types: &'a [TypeId],
    index: usize,
}

impl<'a> Iterator for TupleIter<'a> {
    type Item = Value<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let &ty_id = self.types.get(self.index)?;
        self.index += 1;
        self.cursor.next_value(ty_id).ok()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.types.len() - self.index;
        (rem, Some(rem))
    }
}

impl ExactSizeIterator for TupleIter<'_> {}

// Serialize impl

use serde::ser::Error as _;

impl Serialize for Value<'_> {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut data: &[u8] = self.data;
        let ty = self
            .registry
            .resolve(self.ty_id)
            .ok_or_else(|| S::Error::custom(Error::TypeNotFound(self.ty_id)))?;

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
                let inner = self
                    .registry
                    .resolve(*inner_ty)
                    .ok_or_else(|| S::Error::custom(Error::TypeNotFound(*inner_ty)))?;
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
                let mut cursor = Cursor::new(data, self.registry);
                let mut seq = ser.serialize_seq(Some(len))?;
                for _ in 0..len {
                    seq.serialize_element(&cursor.next_value(*ty_id).map_err(S::Error::custom)?)?;
                }
                seq.end()
            }
            TypeDef::Map(ty_k, ty_v) => {
                let (len, p_size) = sequence_size(data).map_err(S::Error::custom)?;
                data.advance(p_size);
                let mut cursor = Cursor::new(data, self.registry);
                let mut state = ser.serialize_map(Some(len))?;
                for _ in 0..len {
                    let key = cursor.next_value(*ty_k).map_err(S::Error::custom)?;
                    let val = cursor.next_value(*ty_v).map_err(S::Error::custom)?;
                    state.serialize_entry(&key, &val)?;
                }
                state.end()
            }
            TypeDef::Array(ty_id, len) => {
                let mut cursor = Cursor::new(data, self.registry);
                let mut state = ser.serialize_tuple(*len as usize)?;
                for _ in 0..*len {
                    state
                        .serialize_element(&cursor.next_value(*ty_id).map_err(S::Error::custom)?)?;
                }
                state.end()
            }
            TypeDef::Tuple(fields) => {
                let mut cursor = Cursor::new(data, self.registry);
                let mut state = ser.serialize_tuple(fields.len())?;
                for ty_id in fields {
                    state
                        .serialize_element(&cursor.next_value(*ty_id).map_err(S::Error::custom)?)?;
                }
                state.end()
            }
            TypeDef::Struct(fields) => {
                let mut cursor = Cursor::new(data, self.registry);
                let mut state = ser.serialize_map(Some(fields.len()))?;
                for f in fields {
                    state.serialize_key(&f.name)?;
                    state.serialize_value(&cursor.next_value(f.ty).map_err(S::Error::custom)?)?;
                }
                state.end()
            }
            TypeDef::StructUnit => ser.serialize_unit(),
            TypeDef::StructNewType(ty_id) => {
                let mut cursor = Cursor::new(data, self.registry);
                ser.serialize_newtype_struct(
                    "",
                    &cursor.next_value(*ty_id).map_err(S::Error::custom)?,
                )
            }
            TypeDef::StructTuple(fields) => {
                let mut cursor = Cursor::new(data, self.registry);
                let mut state = ser.serialize_tuple_struct("", fields.len())?;
                for ty_id in fields {
                    state.serialize_field(&cursor.next_value(*ty_id).map_err(S::Error::custom)?)?;
                }
                state.end()
            }
            TypeDef::Variant(vdef) => {
                if data.is_empty() {
                    return Err(S::Error::custom(Error::Eof));
                }
                let idx = data.get_u8();
                let var = vdef.variant(idx).map_err(S::Error::custom)?;
                let is_option = vdef.name == "Option";
                let mut cursor = Cursor::new(data, self.registry);

                match &var.fields {
                    Fields::Unit => {
                        if is_option && var.name == "None" {
                            ser.serialize_none()
                        } else {
                            ser.serialize_str(&var.name)
                        }
                    }
                    Fields::NewType(ty_id) => {
                        let v = cursor.next_value(*ty_id).map_err(S::Error::custom)?;
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
                            .map(|ty| cursor.next_value(*ty).map_err(S::Error::custom))
                            .collect();
                        s.serialize_value(&vals?)?;
                        s.end()
                    }
                    Fields::Struct(fields) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(&var.name)?;
                        s.serialize_value(&StructFields(&mut cursor, fields))?;
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

/// Helper to serialize variant struct fields as a map without intermediate allocation.
struct StructFields<'a, 'b>(&'b mut Cursor<'a>, &'a [Field]);

impl Serialize for StructFields<'_, '_> {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        // We need &mut access to the cursor, but Serialize takes &self.
        // Since this is only ever serialized once, we use a cell to get interior mutability.
        // Actually we can't do that cleanly, fall back to collecting.
        // TODO: consider a custom Serializer approach to avoid this allocation.
        let mut data = self.0.remaining();
        let mut state = ser.serialize_map(Some(self.1.len()))?;
        for f in self.1 {
            state.serialize_key(&f.name)?;
            let size = ty_size(self.0.registry, data, f.ty).map_err(S::Error::custom)?;
            let (chunk, rest) = data.split_at(size);
            data = rest;
            state.serialize_value(&Value::new(chunk, f.ty, self.0.registry))?;
        }
        state.end()
    }
}

#[inline]
fn compact_size(data: &[u8]) -> Result<usize, Error> {
    let &first = data.first().ok_or(Error::Eof)?;
    match first & 0b11 {
        0 => Ok(1),
        1 if data.len() >= 2 => Ok(2),
        2 if data.len() >= 4 => Ok(4),
        3 => {
            let total = 1 + (first >> 2) as usize + 4;
            (data.len() >= total).then_some(total).ok_or(Error::Eof)
        }
        _ => Err(Error::Eof),
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
            self.ty()
        )
    }
}

#[cfg(all(feature = "json", not(feature = "text")))]
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
