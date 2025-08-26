use crate::{EnumVariant, SpecificType};
use alloc::{collections::BTreeMap, vec::Vec};
use bytes::{Buf, Bytes};
use codec::Encode;
use core::{convert::TryInto, mem, str};
use scale_info::{prelude::*, PortableRegistry, TypeDefPrimitive as Primitive};
use serde::ser::{SerializeMap, SerializeSeq, SerializeTuple, SerializeTupleStruct};
use serde::Serialize;

type Type = scale_info::Type<scale_info::form::PortableForm>;
type TypeId = u32;
type TypeDef = scale_info::TypeDef<scale_info::form::PortableForm>;

/// A container for SCALE encoded data that can serialize types directly
/// with the help of a type registry and without using an intermediate representation.
pub struct Value<'a> {
    data: Bytes,
    ty_id: TypeId,
    registry: &'a PortableRegistry,
}

impl<'a> Value<'a> {
    pub fn new(data: impl Into<Bytes>, ty_id: u32, registry: &'a PortableRegistry) -> Self {
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
    fn resolve(&self, ty: TypeId) -> &'a Type {
        self.registry.resolve(ty).expect("in registry")
    }

    pub fn size(&self) -> usize {
        self.ty_size(&self.data, self.ty_id)
    }

    fn ty_size(&self, data: &[u8], ty: TypeId) -> usize {
        match &self.resolve(ty).type_def {
            TypeDef::Primitive(ref p) => match p {
                Primitive::U8 => mem::size_of::<u8>(),
                Primitive::U16 => mem::size_of::<u16>(),
                Primitive::U32 => mem::size_of::<u32>(),
                Primitive::U64 => mem::size_of::<u64>(),
                Primitive::U128 => mem::size_of::<u128>(),
                Primitive::I8 => mem::size_of::<i8>(),
                Primitive::I16 => mem::size_of::<i16>(),
                Primitive::I32 => mem::size_of::<i32>(),
                Primitive::I64 => mem::size_of::<i64>(),
                Primitive::I128 => mem::size_of::<i128>(),
                Primitive::Bool => mem::size_of::<bool>(),
                Primitive::Char => mem::size_of::<char>(),
                Primitive::Str => {
                    let (l, p_size) = sequence_size(data);
                    l + p_size
                }
                _ => unimplemented!(),
            },
            TypeDef::Composite(c) => c
                .fields
                .iter()
                .fold(0, |c, f| c + self.ty_size(&data[c..], f.ty.id)),
            TypeDef::Variant(e) => {
                let var = e
                    .variants
                    .iter()
                    .find(|v| v.index == data[0])
                    .expect("variant");

                if var.fields.is_empty() {
                    1 // unit variant
                } else {
                    var.fields
                        .iter()
                        .fold(1, |c, f| c + self.ty_size(&data[c..], f.ty.id))
                }
            }
            TypeDef::Sequence(s) => {
                let (len, prefix_size) = sequence_size(data);
                let ty_id = s.type_param.id;
                (0..len).fold(prefix_size, |c, _| c + self.ty_size(&data[c..], ty_id))
            }
            TypeDef::Array(a) => a.len.try_into().unwrap(),
            TypeDef::Tuple(t) => t
                .fields
                .iter()
                .fold(0, |c, f| c + self.ty_size(&data[c..], f.id)),
            TypeDef::Compact(_) => compact_size(data),
            TypeDef::BitSequence(_) => unimplemented!(),
        }
    }

    /// Extract a `u8` value if this Value represents a u8
    pub fn as_u8(&self) -> Option<u8> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::U8) => {
                if self.data.len() >= mem::size_of::<u8>() {
                    Some(self.data[0])
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract a `u16` value if this Value represents a u16
    pub fn as_u16(&self) -> Option<u16> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::U16) => {
                if self.data.len() >= mem::size_of::<u16>() {
                    Some(u16::from_le_bytes([self.data[0], self.data[1]]))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract a `u32` value if this Value represents a u32
    pub fn as_u32(&self) -> Option<u32> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::U32) => {
                if self.data.len() >= mem::size_of::<u32>() {
                    let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                    Some(u32::from_le_bytes(bytes))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract a `u64` value if this Value represents a u64
    pub fn as_u64(&self) -> Option<u64> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::U64) => {
                if self.data.len() >= mem::size_of::<u64>() {
                    let bytes: [u8; 8] = self.data[0..8].try_into().ok()?;
                    Some(u64::from_le_bytes(bytes))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract a `u128` value if this Value represents a u128
    pub fn as_u128(&self) -> Option<u128> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::U128) => {
                if self.data.len() >= mem::size_of::<u128>() {
                    let bytes: [u8; 16] = self.data[0..16].try_into().ok()?;
                    Some(u128::from_le_bytes(bytes))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract an `i8` value if this Value represents an i8
    pub fn as_i8(&self) -> Option<i8> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::I8) => {
                if self.data.len() >= mem::size_of::<i8>() {
                    Some(self.data[0] as i8)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract an `i16` value if this Value represents an i16
    pub fn as_i16(&self) -> Option<i16> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::I16) => {
                if self.data.len() >= mem::size_of::<i16>() {
                    Some(i16::from_le_bytes([self.data[0], self.data[1]]))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract an `i32` value if this Value represents an i32
    pub fn as_i32(&self) -> Option<i32> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::I32) => {
                if self.data.len() >= mem::size_of::<i32>() {
                    let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                    Some(i32::from_le_bytes(bytes))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract an `i64` value if this Value represents an i64
    pub fn as_i64(&self) -> Option<i64> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::I64) => {
                if self.data.len() >= mem::size_of::<i64>() {
                    let bytes: [u8; 8] = self.data[0..8].try_into().ok()?;
                    Some(i64::from_le_bytes(bytes))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract an `i128` value if this Value represents an i128
    pub fn as_i128(&self) -> Option<i128> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::I128) => {
                if self.data.len() >= mem::size_of::<i128>() {
                    let bytes: [u8; 16] = self.data[0..16].try_into().ok()?;
                    Some(i128::from_le_bytes(bytes))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract a `bool` value if this Value represents a bool
    pub fn as_bool(&self) -> Option<bool> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::Bool) => {
                if self.data.len() >= mem::size_of::<bool>() {
                    Some(self.data[0] != 0)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract a `char` value if this Value represents a char
    pub fn as_char(&self) -> Option<char> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::Char) => {
                if self.data.len() >= mem::size_of::<char>() {
                    let bytes: [u8; 4] = self.data[0..4].try_into().ok()?;
                    char::from_u32(u32::from_le_bytes(bytes))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract a `&str` value if this Value represents a string
    pub fn as_str(&self) -> Option<&str> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Primitive(Primitive::Str) => {
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

    /// Check if this Value represents a primitive u8
    pub fn is_u8(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::U8)
        )
    }

    /// Check if this Value represents a primitive u16
    pub fn is_u16(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::U16)
        )
    }

    /// Check if this Value represents a primitive u32
    pub fn is_u32(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::U32)
        )
    }

    /// Check if this Value represents a primitive u64
    pub fn is_u64(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::U64)
        )
    }

    /// Check if this Value represents a primitive u128
    pub fn is_u128(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::U128)
        )
    }

    /// Check if this Value represents a primitive i8
    pub fn is_i8(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::I8)
        )
    }

    /// Check if this Value represents a primitive i16
    pub fn is_i16(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::I16)
        )
    }

    /// Check if this Value represents a primitive i32
    pub fn is_i32(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::I32)
        )
    }

    /// Check if this Value represents a primitive i64
    pub fn is_i64(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::I64)
        )
    }

    /// Check if this Value represents a primitive i128
    pub fn is_i128(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::I128)
        )
    }

    /// Check if this Value represents a primitive bool
    pub fn is_bool(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::Bool)
        )
    }

    /// Check if this Value represents a primitive char
    pub fn is_char(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::Char)
        )
    }

    /// Check if this Value represents a primitive string
    pub fn is_string(&self) -> bool {
        matches!(
            &self.resolve(self.ty_id).type_def,
            TypeDef::Primitive(Primitive::Str)
        )
    }

    /// Check if this Value represents a sequence/array
    pub fn is_sequence(&self) -> bool {
        matches!(&self.resolve(self.ty_id).type_def, TypeDef::Sequence(_))
    }

    /// Check if this Value represents an array
    pub fn is_array(&self) -> bool {
        matches!(&self.resolve(self.ty_id).type_def, TypeDef::Array(_))
    }

    /// Check if this Value represents a tuple
    pub fn is_tuple(&self) -> bool {
        matches!(&self.resolve(self.ty_id).type_def, TypeDef::Tuple(_))
    }

    /// Check if this Value represents a composite (struct)
    pub fn is_composite(&self) -> bool {
        matches!(&self.resolve(self.ty_id).type_def, TypeDef::Composite(_))
    }

    /// Check if this Value represents a variant (enum)
    pub fn is_variant(&self) -> bool {
        matches!(&self.resolve(self.ty_id).type_def, TypeDef::Variant(_))
    }

    /// Check if this Value represents a compact-encoded value
    pub fn is_compact(&self) -> bool {
        matches!(&self.resolve(self.ty_id).type_def, TypeDef::Compact(_))
    }

    /// Get the length of a sequence if this Value represents a sequence
    pub fn sequence_len(&self) -> Option<usize> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Sequence(_) => {
                let (len, _) = sequence_size(&self.data);
                Some(len)
            }
            _ => None,
        }
    }

    /// Get an element from a sequence by index
    pub fn sequence_get(&self, index: usize) -> Option<Value<'a>> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Sequence(s) => {
                let (len, prefix_size) = sequence_size(&self.data);
                if index >= len {
                    return None;
                }

                let mut data = self.data.slice(prefix_size..);
                let ty_id = s.type_param.id;

                // Skip to the desired index
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
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Array(a) => Some(a.len),
            _ => None,
        }
    }

    /// Get an element from an array by index
    pub fn array_get(&self, index: u32) -> Option<Value<'a>> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Array(a) => {
                if index >= a.len {
                    return None;
                }

                let ty_id = a.type_param.id;
                let element_size = self.ty_size(&self.data, ty_id);
                let start_offset = (index as usize) * element_size;
                let end_offset = start_offset + element_size;

                if end_offset <= self.data.len() {
                    let element_data = self.data.slice(start_offset..end_offset);
                    Some(Value::new(element_data, ty_id, self.registry))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get a field from a composite type by index
    pub fn field_at(&self, index: usize) -> Option<Value<'a>> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Composite(c) => {
                if index >= c.fields.len() {
                    return None;
                }

                let data = self.data.clone();
                let mut offset = 0;

                // Skip to the desired field
                for i in 0..index {
                    let field_ty_id = c.fields[i].ty.id;
                    let field_size = self.ty_size(&data[offset..], field_ty_id);
                    offset += field_size;
                }

                let field_ty_id = c.fields[index].ty.id;
                let field_size = self.ty_size(&data[offset..], field_ty_id);
                let field_data = data.slice(offset..offset + field_size);

                Some(Value::new(field_data, field_ty_id, self.registry))
            }
            _ => None,
        }
    }

    /// Get a field from a composite type by name
    pub fn field(&self, name: &str) -> Option<Value<'a>> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Composite(c) => {
                let field_index = c
                    .fields
                    .iter()
                    .position(|f| f.name.as_deref() == Some(name))?;
                self.field_at(field_index)
            }
            _ => None,
        }
    }

    /// Get the number of fields in a composite type
    pub fn field_count(&self) -> Option<usize> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Composite(c) => Some(c.fields.len()),
            _ => None,
        }
    }

    /// Get an element from a tuple by index
    pub fn tuple_get(&self, index: usize) -> Option<Value<'a>> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Tuple(t) => {
                if index >= t.fields.len() {
                    return None;
                }

                let data = self.data.clone();
                let mut offset = 0;

                // Skip to the desired element
                for i in 0..index {
                    let elem_ty_id = t.fields[i].id;
                    let elem_size = self.ty_size(&data[offset..], elem_ty_id);
                    offset += elem_size;
                }

                let elem_ty_id = t.fields[index].id;
                let elem_size = self.ty_size(&data[offset..], elem_ty_id);
                let elem_data = data.slice(offset..offset + elem_size);

                Some(Value::new(elem_data, elem_ty_id, self.registry))
            }
            _ => None,
        }
    }

    /// Get the number of elements in a tuple
    pub fn tuple_len(&self) -> Option<usize> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Tuple(t) => Some(t.fields.len()),
            _ => None,
        }
    }

    /// Get the variant index if this Value represents a variant (enum)
    pub fn variant_index(&self) -> Option<u8> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Variant(_) => {
                if self.data.len() > 0 {
                    Some(self.data[0])
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get the variant name if this Value represents a variant (enum)
    pub fn variant_name(&self) -> Option<&str> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Variant(e) => {
                if self.data.len() > 0 {
                    let variant_index = self.data[0];
                    e.variants
                        .iter()
                        .find(|v| v.index == variant_index)
                        .map(|v| v.name.as_str())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get the variant data if this Value represents a variant (enum) with data
    pub fn variant_data(&self) -> Option<Value<'a>> {
        match &self.resolve(self.ty_id).type_def {
            TypeDef::Variant(e) => {
                if self.data.len() > 0 {
                    let variant_index = self.data[0];
                    let variant = e.variants.iter().find(|v| v.index == variant_index)?;

                    if variant.fields.is_empty() {
                        None // Unit variant has no data
                    } else if variant.fields.len() == 1 {
                        // Single field variant
                        let field_ty_id = variant.fields[0].ty.id;
                        let variant_data = self.data.slice(1..);
                        Some(Value::new(variant_data, field_ty_id, self.registry))
                    } else {
                        // Multi-field variant - create a tuple-like structure
                        let _variant_data = self.data.slice(1..);
                        // We would need to construct a synthetic tuple type here
                        // For now, return None as this is complex
                        None
                    }
                } else {
                    None
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

        use SpecificType::*;
        match (ty, self.registry).into() {
            Bool => ser.serialize_bool(data.get_u8() != 0),
            U8 => ser.serialize_u8(data.get_u8()),
            U16 => ser.serialize_u16(data.get_u16_le()),
            U32 => ser.serialize_u32(data.get_u32_le()),
            U64 => ser.serialize_u64(data.get_u64_le()),
            U128 => ser.serialize_u128(data.get_u128_le()),
            I8 => ser.serialize_i8(data.get_i8()),
            I16 => ser.serialize_i16(data.get_i16_le()),
            I32 => ser.serialize_i32(data.get_i32_le()),
            I64 => ser.serialize_i64(data.get_i64_le()),
            I128 => ser.serialize_i128(data.get_i128_le()),
            Compact(ty) => {
                let type_def = &self
                    .registry
                    .resolve(ty)
                    .expect("not found in registry")
                    .type_def;

                use codec::Compact;
                match type_def {
                    TypeDef::Primitive(Primitive::U32) => {
                        ser.serialize_bytes(&Compact(data.get_u32_le()).encode())
                    }
                    TypeDef::Primitive(Primitive::U64) => {
                        ser.serialize_bytes(&Compact(data.get_u64_le()).encode())
                    }
                    TypeDef::Primitive(Primitive::U128) => {
                        ser.serialize_bytes(&Compact(data.get_u128_le()).encode())
                    }
                    _ => unimplemented!(),
                }
            }
            Bytes(_) => {
                let (_, s) = sequence_size(data.chunk());
                data.advance(s);
                ser.serialize_bytes(data.chunk())
            }
            Char => ser.serialize_char(char::from_u32(data.get_u32_le()).unwrap()),
            Str => {
                let (_, s) = sequence_size(data.chunk());
                data.advance(s);
                ser.serialize_str(str::from_utf8(data.chunk()).unwrap())
            }
            Sequence(ty) => {
                let (len, p_size) = sequence_size(data.chunk());
                data.advance(p_size);

                let mut seq = ser.serialize_seq(Some(len))?;
                for _ in 0..len {
                    seq.serialize_element(&self.new_value(&mut data, ty))?;
                }
                seq.end()
            }
            Map(ty_k, ty_v) => {
                let (len, p_size) = sequence_size(data.chunk());
                data.advance(p_size);

                let mut state = ser.serialize_map(Some(len))?;
                for _ in 0..len {
                    let key = self.new_value(&mut data, ty_k);
                    let val = self.new_value(&mut data, ty_v);
                    state.serialize_entry(&key, &val)?;
                }
                state.end()
            }
            Tuple(t) => {
                let mut state = ser.serialize_tuple(t.len())?;
                for i in 0..t.len() {
                    state.serialize_element(&self.new_value(&mut data, t.type_id(i)))?;
                }
                state.end()
            }
            Struct(fields) => {
                let mut state = ser.serialize_map(Some(fields.len()))?;
                for (name, ty) in fields {
                    state.serialize_key(&name)?;
                    state.serialize_value(&self.new_value(&mut data, ty))?;
                }
                state.end()
            }
            StructUnit => ser.serialize_unit(),
            StructNewType(ty) => ser.serialize_newtype_struct("", &self.new_value(&mut data, ty)),
            StructTuple(fields) => {
                let mut state = ser.serialize_tuple_struct("", fields.len())?;
                for ty in fields {
                    state.serialize_field(&self.new_value(&mut data, ty))?;
                }
                state.end()
            }
            ty @ Variant(_, _, _) => {
                let variant = &ty.pick(data.get_u8());
                match variant.into() {
                    EnumVariant::OptionNone => ser.serialize_none(),
                    EnumVariant::OptionSome(ty) => {
                        ser.serialize_some(&self.new_value(&mut data, ty))
                    }
                    EnumVariant::Unit(_idx, name) => ser.serialize_str(name),
                    EnumVariant::NewType(_idx, name, ty) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(name)?;
                        s.serialize_value(&self.new_value(&mut data, ty))?;
                        s.end()
                    }

                    EnumVariant::Tuple(_idx, name, fields) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(name)?;
                        s.serialize_value(
                            &fields
                                .iter()
                                .map(|ty| self.new_value(&mut data, *ty))
                                .collect::<Vec<_>>(),
                        )?;
                        s.end()
                    }
                    EnumVariant::Struct(_idx, name, fields) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(name)?;
                        s.serialize_value(&fields.iter().fold(
                            BTreeMap::new(),
                            |mut m, (name, ty)| {
                                m.insert(*name, self.new_value(&mut data, *ty));
                                m
                            },
                        ))?;
                        s.end()
                    }
                }
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
    // need to peek at the data to know the length of sequence
    // first byte(s) gives us a hint of the(compact encoded) length
    // https://substrate.dev/docs/en/knowledgebase/advanced/codec#compactgeneral-integers
    let len = compact_size(data);
    (
        match len {
            1 => (data[0] >> 2).into(),
            2 => u16::from((data[0] >> 2) | (data[1] << 6)).into(),
            4 => (((data[0] as u32) >> 2)
                | ((data[1] as u32) << 6)
                | ((data[2] as u32) << 14)
                | ((data[3] as u32) << 22))
                .try_into()
                .unwrap(),
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

#[cfg(feature = "codec")]
impl codec::Encode for Value<'_> {
    fn size_hint(&self) -> usize {
        self.data.len()
    }
    fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
        f(self.data.as_ref())
    }
}

impl core::fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Value {{ data: {:?}, type({}): {:?} }}",
            self.data,
            self.ty_id,
            self.registry.resolve(self.ty_id).unwrap().type_def
        )
    }
}

#[cfg(feature = "json")]
impl core::fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self).map_err(|_| fmt::Error)?
        )
    }
}

#[cfg(feature = "json")]
impl<'reg> From<Value<'reg>> for crate::JsonValue {
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
        Registry, TypeInfo,
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

    fn register<T>(_ty: &T) -> (u32, PortableRegistry)
    where
        T: TypeInfo + 'static,
    {
        let mut reg = Registry::new();
        let sym = reg.register_type(&meta_type::<T>());
        (sym.id, reg.into())
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

    #[cfg(feature = "codec")]
    #[test]
    fn encodable() {
        let input = u8::MAX;
        let (ty, reg) = register(&input);
        let value = Value::new(b"1234".as_ref(), ty, &reg);

        let expected: &[u8] = value.as_ref();
        assert_eq!(value.encode(), expected);
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
    fn serialize_bool() -> Result<(), Error> {
        let in_value = true;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    // `char` not supported?
    // #[test]
    // fn serialize_char() -> Result<(), Error> {
    //     let extract_value = '⚖';
    //     let data = extract_value.encode();
    //     let info = char::type_info();
    //     let val = Value::new(data, info, reg);
    //     assert_eq!(to_value(val)?, to_value(extract_value)?);
    //     Ok(())
    // }

    #[test]
    fn serialize_u8array() -> Result<(), Error> {
        let in_value: Vec<u8> = [2u8; u8::MAX as usize].into();
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16array() -> Result<(), Error> {
        let in_value: Vec<u16> = [2u16, u16::MAX].into();
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32array() -> Result<(), Error> {
        let in_value: Vec<u32> = [2u32, u32::MAX].into();
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple() -> Result<(), Error> {
        let in_value: (i64, Vec<String>, bool) = (
            i64::MIN,
            vec!["hello".into(), "big".into(), "world".into()],
            true,
        );
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u32struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u32,
            baz: u32,
        }
        let in_value = Foo {
            bar: 123,
            baz: u32::MAX,
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u8struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u8,
            baz: u8,
        }
        let in_value = Foo {
            bar: 123,
            baz: u8::MAX,
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u64struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u64,
            baz: u64,
        }
        let in_value = Foo {
            bar: 123,
            baz: u64::MAX,
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_map() -> Result<(), Error> {
        let in_value = {
            let mut m = BTreeMap::<String, i32>::new();
            m.insert("foo".into(), i32::MAX);
            m.insert("bar".into(), i32::MIN);
            m
        };

        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_complex_struct_with_enum() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        enum Bar {
            This,
            That(i16),
        }
        #[derive(Encode, Serialize, TypeInfo)]
        struct Baz(String);
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: Vec<Bar>,
            baz: Option<Baz>,
            lol: &'static [u8],
        }
        let in_value = Foo {
            bar: [Bar::That(i16::MAX), Bar::This].into(),
            baz: Some(Baz("aliquam malesuada bibendum arcu vitae".into())),
            lol: b"\0xFFsome stuff\0x00",
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple_struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo([u8; 4], (bool, Option<()>), Baz, Baz);

        #[derive(Encode, Serialize, TypeInfo)]
        struct Bar;

        #[derive(Encode, Serialize, TypeInfo)]
        enum Baz {
            A(Bar),
            B { bb: &'static str },
        }

        let in_value = Foo(
            [1, 2, 3, 4],
            (false, None),
            Baz::A(Bar),
            Baz::B { bb: "lol" },
        );
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn test_primitive_extraction() -> Result<(), Error> {
        // Test u8 extraction
        let u8_value = 42u8;
        let data = u8_value.encode();
        let (id, reg) = register(&u8_value);
        let value = Value::new(data, id, &reg);

        assert!(value.is_u8());
        assert_eq!(value.as_u8(), Some(42));
        assert_eq!(value.as_u16(), None); // Wrong type

        // Test u32 extraction
        let u32_value = 123456u32;
        let data = u32_value.encode();
        let (id, reg) = register(&u32_value);
        let value = Value::new(data, id, &reg);

        assert!(value.is_u32());
        assert_eq!(value.as_u32(), Some(123456));
        assert_eq!(value.as_u8(), None); // Wrong type

        // Test bool extraction
        let bool_value = true;
        let data = bool_value.encode();
        let (id, reg) = register(&bool_value);
        let value = Value::new(data, id, &reg);

        assert!(value.is_bool());
        assert_eq!(value.as_bool(), Some(true));

        Ok(())
    }

    #[test]
    fn test_sequence_operations() -> Result<(), Error> {
        let vec_value = vec![10u32, 20u32, 30u32];
        let data = vec_value.encode();
        let (id, reg) = register(&vec_value);
        let value = Value::new(data, id, &reg);

        assert!(value.is_sequence());
        assert_eq!(value.sequence_len(), Some(3));

        // Test getting elements
        let first_elem = value.sequence_get(0).unwrap();
        assert!(first_elem.is_u32());
        assert_eq!(first_elem.as_u32(), Some(10));

        let second_elem = value.sequence_get(1).unwrap();
        assert_eq!(second_elem.as_u32(), Some(20));

        // Test out of bounds
        assert!(value.sequence_get(5).is_none());

        Ok(())
    }

    #[test]
    fn test_composite_field_access() -> Result<(), Error> {
        #[derive(Encode, scale_info::TypeInfo)]
        struct TestStruct {
            field_a: u32,
            field_b: bool,
            field_c: u16,
        }

        let test_struct = TestStruct {
            field_a: 42,
            field_b: true,
            field_c: 1000,
        };

        let data = test_struct.encode();
        let (id, reg) = register(&test_struct);
        let value = Value::new(data, id, &reg);

        assert!(value.is_composite());
        assert_eq!(value.field_count(), Some(3));

        // Test field access by index
        let field_a = value.field_at(0).unwrap();
        assert!(field_a.is_u32());
        assert_eq!(field_a.as_u32(), Some(42));

        let field_b = value.field_at(1).unwrap();
        assert!(field_b.is_bool());
        assert_eq!(field_b.as_bool(), Some(true));

        let field_c = value.field_at(2).unwrap();
        assert!(field_c.is_u16());
        assert_eq!(field_c.as_u16(), Some(1000));

        // Test out of bounds
        assert!(value.field_at(5).is_none());

        Ok(())
    }

    #[test]
    fn test_tuple_operations() -> Result<(), Error> {
        let tuple_value = (42u32, true, 1000u16);
        let data = tuple_value.encode();
        let (id, reg) = register(&tuple_value);
        let value = Value::new(data, id, &reg);

        assert!(value.is_tuple());
        assert_eq!(value.tuple_len(), Some(3));

        // Test tuple element access
        let elem_0 = value.tuple_get(0).unwrap();
        assert!(elem_0.is_u32());
        assert_eq!(elem_0.as_u32(), Some(42));

        let elem_1 = value.tuple_get(1).unwrap();
        assert!(elem_1.is_bool());
        assert_eq!(elem_1.as_bool(), Some(true));

        let elem_2 = value.tuple_get(2).unwrap();
        assert!(elem_2.is_u16());
        assert_eq!(elem_2.as_u16(), Some(1000));

        // Test out of bounds
        assert!(value.tuple_get(5).is_none());

        Ok(())
    }

    #[test]
    fn test_variant_operations() -> Result<(), Error> {
        #[derive(Encode, scale_info::TypeInfo)]
        enum TestEnum {
            Unit,
            WithData(u32),
        }

        // Test unit variant
        let unit_variant = TestEnum::Unit;
        let data = unit_variant.encode();
        let (id, reg) = register(&unit_variant);
        let value = Value::new(data, id, &reg);

        assert!(value.is_variant());
        assert_eq!(value.variant_index(), Some(0));
        assert!(value.variant_data().is_none()); // Unit variant has no data

        // Test variant with data
        let data_variant = TestEnum::WithData(123);
        let data = data_variant.encode();
        let (id, reg) = register(&data_variant);
        let value = Value::new(data, id, &reg);

        assert!(value.is_variant());
        assert_eq!(value.variant_index(), Some(1));

        let variant_data = value.variant_data().unwrap();
        assert!(variant_data.is_u32());
        assert_eq!(variant_data.as_u32(), Some(123));

        Ok(())
    }

    #[test]
    fn test_string_extraction() -> Result<(), Error> {
        let string_value = "Hello, World!".to_string();
        let data = string_value.encode();
        let (id, reg) = register(&string_value);
        let value = Value::new(data, id, &reg);

        assert!(value.is_string());
        assert_eq!(value.as_str(), Some("Hello, World!"));

        Ok(())
    }
}
