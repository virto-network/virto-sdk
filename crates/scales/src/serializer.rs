use crate::error::Error;
use crate::prelude::*;
use crate::registry::*;
use bytes::BufMut;
use core::fmt::Debug;

use serde::{ser, Serialize};

type Result<T> = core::result::Result<T, Error>;

/// SCALE-encode a value into a new `Vec<u8>`.
#[inline]
pub fn to_vec<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize + ?Sized,
{
    let mut out = vec![];
    to_bytes(&mut out, value)?;
    Ok(out)
}

/// SCALE-encode a value into a new `Vec<u8>`, using type info to coerce representations.
#[inline]
pub fn to_vec_with_info<T>(value: &T, registry_type: Option<(&Registry, TypeId)>) -> Result<Vec<u8>>
where
    T: Serialize + ?Sized,
{
    let mut out = vec![];
    to_bytes_with_info(&mut out, value, registry_type)?;
    Ok(out)
}

/// SCALE-encode a value into an existing buffer.
pub fn to_bytes<B, T>(bytes: B, value: &T) -> Result<()>
where
    T: Serialize + ?Sized,
    B: BufMut + Debug,
{
    to_bytes_with_info(bytes, value, None)
}

/// SCALE-encode a value into a buffer, using type info to coerce representations.
pub fn to_bytes_with_info<B, T>(
    bytes: B,
    value: &T,
    registry_type: Option<(&Registry, TypeId)>,
) -> Result<()>
where
    T: Serialize + ?Sized,
    B: BufMut + Debug,
{
    let mut serializer = Serializer::new(bytes, registry_type);
    value.serialize(&mut serializer)?;
    Ok(())
}

/// SCALE-encode a JSON-like key-value iterator into a buffer using type info.
#[cfg(feature = "json")]
pub fn to_bytes_from_iter<B, K, V>(
    bytes: B,
    iter: impl IntoIterator<Item = (K, V)>,
    registry_type: (&Registry, TypeId),
) -> Result<()>
where
    B: BufMut + Debug,
    K: Into<String>,
    V: Into<crate::JsonValue>,
{
    let ty = registry_type
        .0
        .resolve(registry_type.1)
        .ok_or_else(|| Error::BadInput("Type not in registry".into()))?;
    let obj = iter.into_iter().collect::<crate::JsonValue>();
    let val: crate::JsonValue = if let TypeDef::Struct(fields) = ty {
        fields
            .iter()
            .map(|f| {
                Ok((
                    f.name,
                    obj.get(f.name)
                        .ok_or_else(|| Error::BadInput(format!("missing field {}", f.name)))?
                        .clone(),
                ))
            })
            .collect::<Result<_>>()?
    } else {
        return Err(Error::BadType(format!("{:?}", ty)));
    };

    to_bytes_with_info(bytes, &val, Some(registry_type))
}

/// Like [`to_bytes_from_iter`] but allocates and returns a `Vec<u8>`.
#[cfg(feature = "json")]
pub fn to_vec_from_iter<I, K, V>(iter: I, registry_type: (&Registry, TypeId)) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<crate::JsonValue>,
{
    let mut out = vec![];
    to_bytes_from_iter(&mut out, iter, registry_type)?;
    Ok(out)
}

/// A serializer that encodes types to SCALE with the option to coerce
/// the output to an equivalent representation given by some type information.
#[derive(Debug)]
pub struct Serializer<'reg, B>
where
    B: Debug,
{
    out: B,
    ty_id: Option<TypeId>,
    /// Synthetic type override for cases where no registry TypeId exists (e.g. ByteSeq U8 elements)
    ty_hint: Option<TypeDef<'reg>>,
    registry: Option<&'reg Registry>,
    picked: Option<usize>,
}

impl<'reg, B> Serializer<'reg, B>
where
    B: BufMut + Debug,
{
    pub fn new(out: B, registry_type: Option<(&'reg Registry, TypeId)>) -> Self {
        // A newtype encodes as its inner type; unwrap the root like field
        // types, or e.g. a newtype around an `Option` would be serialized
        // without type information.
        let (registry, ty_id) = match registry_type {
            Some((reg, ty_id)) => (Some(reg), Some(unwrap_newtype(reg, ty_id))),
            None => (None, None),
        };
        Serializer {
            out,
            ty_id,
            ty_hint: None,
            registry,
            picked: None,
        }
    }

    fn serialize_compact(&mut self, _ty: u32, v: u128) -> Result<()> {
        crate::compact_encode(v, &mut self.out);
        Ok(())
    }
}

impl<'a, 'reg, B> ser::Serializer for &'a mut Serializer<'reg, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    type SerializeSeq = TypedSerializer<'a, 'reg, B>;
    type SerializeTuple = TypedSerializer<'a, 'reg, B>;
    type SerializeTupleStruct = TypedSerializer<'a, 'reg, B>;
    type SerializeTupleVariant = TypedSerializer<'a, 'reg, B>;
    type SerializeMap = TypedSerializer<'a, 'reg, B>;
    type SerializeStruct = TypedSerializer<'a, 'reg, B>;
    type SerializeStructVariant = TypedSerializer<'a, 'reg, B>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok> {
        self.maybe_some()?;
        self.out.put_u8(v.into());
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok> {
        self.maybe_some()?;
        self.out.put_i8(v);
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok> {
        self.maybe_some()?;
        self.out.put_i16_le(v);
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok> {
        self.maybe_some()?;
        self.out.put_i32_le(v);
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok> {
        match self.ty() {
            Some(TypeDef::I8) => self.serialize_i8(v as i8)?,
            Some(TypeDef::I16) => self.serialize_i16(v as i16)?,
            Some(TypeDef::I32) => self.serialize_i32(v as i32)?,
            _ => {
                self.maybe_some()?;
                self.out.put_i64_le(v)
            }
        }
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok> {
        self.maybe_some()?;
        self.out.put_u8(v);
        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok> {
        self.maybe_some()?;
        self.out.put_u16_le(v);
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok> {
        self.maybe_some()?;
        self.out.put_u32_le(v);
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok> {
        self.maybe_some()?;
        // all numbers in serde_json are the same
        match self.ty() {
            Some(TypeDef::I8) => self.serialize_i8(v as i8)?,
            Some(TypeDef::I16) => self.serialize_i16(v as i16)?,
            Some(TypeDef::I32) => self.serialize_i32(v as i32)?,
            Some(TypeDef::U8) => self.serialize_u8(v as u8)?,
            Some(TypeDef::U16) => self.serialize_u16(v as u16)?,
            Some(TypeDef::U32) => self.serialize_u32(v as u32)?,
            Some(TypeDef::Compact(ty)) => self.serialize_compact(ty, v as u128)?,
            _ => self.out.put_u64_le(v),
        }
        Ok(())
    }

    fn serialize_u128(self, v: u128) -> Result<Self::Ok> {
        self.maybe_some()?;
        match self.ty() {
            Some(TypeDef::I8) => self.serialize_i8(v as i8)?,
            Some(TypeDef::I16) => self.serialize_i16(v as i16)?,
            Some(TypeDef::I32) => self.serialize_i32(v as i32)?,
            Some(TypeDef::I64) => self.serialize_i64(v as i64)?,
            Some(TypeDef::U8) => self.serialize_u8(v as u8)?,
            Some(TypeDef::U16) => self.serialize_u16(v as u16)?,
            Some(TypeDef::U32) => self.serialize_u32(v as u32)?,
            Some(TypeDef::U64) => self.serialize_u64(v as u64)?,
            Some(TypeDef::Compact(ty)) => self.serialize_compact(ty, v)?,
            _ => self.out.put_u128_le(v),
        }
        Ok(())
    }

    fn serialize_f32(self, _v: f32) -> Result<Self::Ok> {
        unimplemented!()
    }

    fn serialize_f64(self, _v: f64) -> Result<Self::Ok> {
        unimplemented!()
    }

    fn serialize_char(self, _v: char) -> Result<Self::Ok> {
        unimplemented!()
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok> {
        self.maybe_some()?;
        if self.maybe_other(v)?.is_some() {
            return Ok(());
        }

        compact_number(v.len(), &mut self.out);
        self.out.put(v.as_bytes());
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok> {
        self.maybe_some()?;

        compact_number(v.len(), &mut self.out);
        self.out.put(v);
        Ok(())
    }

    fn serialize_none(self) -> Result<Self::Ok> {
        self.out.put_u8(0x00);
        Ok(())
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        // An `Option` type takes the tag and moves to the inner type, so
        // the value isn't tagged a second time.
        if !self.maybe_some()? {
            self.out.put_u8(0x01);
        }
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok> {
        // A null `Option` is `None`.
        if self.is_option() {
            self.out.put_u8(0x00);
            return Ok(());
        }
        self.maybe_some()?;
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok> {
        self.maybe_some()?;
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        __name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok> {
        self.maybe_some()?;
        (variant_index as u8).serialize(self)
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        self.maybe_some()?;
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        __name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        self.maybe_some()?;
        self.out.put_u8(variant_index as u8);
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        self.maybe_some()?;
        if matches!(
            self.ty(),
            None | Some(TypeDef::Bytes) | Some(TypeDef::Sequence(_))
        ) {
            compact_number(len.expect("known length"), &mut self.out);
        }
        Ok(self.into())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        self.maybe_some()?;
        Ok(self.into())
    }

    fn serialize_tuple_struct(
        self,
        __name: &'static str,
        __len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.maybe_some()?;
        Ok(self.into())
    }

    fn serialize_tuple_variant(
        self,
        __name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        __len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        self.maybe_some()?;
        self.out.put_u8(variant_index as u8);
        Ok(self.into())
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        self.maybe_some()?;
        if matches!(self.ty(), None | Some(TypeDef::Map(_, _))) {
            compact_number(len.expect("known length"), &mut self.out);
        }
        Ok(self.into())
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        self.maybe_some()?;
        Ok(self.into())
    }

    fn serialize_struct_variant(
        self,
        __name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        __len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        self.maybe_some()?;
        self.out.put_u8(variant_index as u8);
        Ok(self.into())
    }
}

impl<'reg, B: Debug> Serializer<'reg, B> {
    fn ty(&self) -> Option<TypeDef<'reg>> {
        self.ty_id.map(|id| self.resolve(id)).or(self.ty_hint)
    }

    fn resolve(&self, ty_id: TypeId) -> TypeDef<'reg> {
        let reg = self.registry.expect("called having type");
        reg.resolve(ty_id).expect("in registry")
    }
}

impl<'reg, B> Serializer<'reg, B>
where
    B: BufMut + Debug,
{
    // A check to run for every serialize fn since any type could be an Option::Some
    // if the type info says its an Option assume its Some and extract the inner type
    /// If the current type is an `Option`, write the `Some` tag and move to
    /// the inner type. Returns whether it did.
    fn maybe_some(&mut self) -> Result<bool> {
        if let Some(ty_id) = self.ty_id {
            if let TypeDef::Variant(vdef) = self.resolve(ty_id) {
                if vdef.name() == "Option" {
                    self.ty_id = match vdef.variants().nth(1).map(|v| v.fields()) {
                        Some(Fields::NewType(inner)) => Some(inner),
                        _ => None,
                    };
                    self.out.put_u8(0x01);
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn is_option(&self) -> bool {
        self.ty_id
            .is_some_and(|ty_id| matches!(self.resolve(ty_id), TypeDef::Variant(vdef) if vdef.name() == "Option"))
    }

    #[inline]
    fn maybe_other(&mut self, val: &str) -> Result<Option<()>> {
        let ty = match self.ty() {
            Some(ty) => ty,
            None => return Ok(None),
        };
        match ty {
            TypeDef::Str => Ok(None),
            // { "foo": "Bar" } => "Bar" might be an enum variant
            TypeDef::Variant(vdef) => {
                let variant = vdef
                    .variants()
                    .find(|v| v.name() == val)
                    .ok_or_else(|| Error::BadInput("Invalid variant".into()))?;
                self.out.put_u8(variant.index());
                Ok(Some(()))
            }
            TypeDef::StructNewType(ty_id) => match self.resolve(ty_id) {
                // { "foo": "bar" } => "bar" might be a string wrapped in a type
                TypeDef::Str => Ok(None),
                ty => Err(Error::NotSupported(
                    type_name_of_val(val),
                    format!("{:?}", ty),
                )),
            },
            TypeDef::U8 => {
                let n = val.parse().map_err(|_| Error::BadInput("u8".into()))?;
                self.out.put_u8(n);
                Ok(Some(()))
            }
            TypeDef::U16 => {
                let n = val.parse().map_err(|_| Error::BadInput("u16".into()))?;
                self.out.put_u16_le(n);
                Ok(Some(()))
            }
            TypeDef::U32 => {
                let n = val.parse().map_err(|_| Error::BadInput("u32".into()))?;
                self.out.put_u32_le(n);
                Ok(Some(()))
            }
            TypeDef::U64 => {
                let n = val.parse().map_err(|_| Error::BadInput("u64".into()))?;
                self.out.put_u64_le(n);
                Ok(Some(()))
            }
            TypeDef::U128 => {
                let n = val.parse().map_err(|_| Error::BadInput("u128".into()))?;
                self.out.put_u128_le(n);
                Ok(Some(()))
            }
            TypeDef::I8 => {
                let n = val.parse().map_err(|_| Error::BadInput("i8".into()))?;
                self.out.put_i8(n);
                Ok(Some(()))
            }
            TypeDef::I16 => {
                let n = val.parse().map_err(|_| Error::BadInput("i16".into()))?;
                self.out.put_i16_le(n);
                Ok(Some(()))
            }
            TypeDef::I32 => {
                let n = val.parse().map_err(|_| Error::BadInput("i32".into()))?;
                self.out.put_i32_le(n);
                Ok(Some(()))
            }
            TypeDef::I64 => {
                let n = val.parse().map_err(|_| Error::BadInput("i64".into()))?;
                self.out.put_i64_le(n);
                Ok(Some(()))
            }
            TypeDef::I128 => {
                let n = val.parse().map_err(|_| Error::BadInput("i128".into()))?;
                self.out.put_i128_le(n);
                Ok(Some(()))
            }
            TypeDef::Bytes => {
                if let Some(hex) = val.strip_prefix("0x") {
                    let bytes = crate::decode_hex(hex)?;
                    ser::Serializer::serialize_bytes(self, &bytes)?;
                    Ok(Some(()))
                } else {
                    Err(Error::BadInput("Hex string must start with 0x".into()))
                }
            }
            ty => Err(Error::NotSupported(
                type_name_of_val(val),
                format!("{:?}", ty),
            )),
        }
    }
}

/// Borrowed field type sources — avoids Vec allocation and O(n) remove(0)
#[derive(Debug)]
pub enum FieldTypes<'reg> {
    Ids(&'reg [TypeId]),
}

impl FieldTypes<'_> {
    fn next(&mut self) -> TypeId {
        let FieldTypes::Ids(ids) = self;
        let (first, rest) = ids.split_first().expect("field available");
        *ids = rest;
        *first
    }
}

/// Named struct fields, encoded by name: each value is encoded with its
/// declared type and the fields are written in declaration order, so the
/// input's key order does not matter (e.g. JSON objects sorted by key).
/// Unknown or duplicate names are rejected; a missing `Option` field is
/// `None` and any other missing field is an error.
#[derive(Debug)]
pub struct NamedFields<'reg> {
    fields: alloc::vec::Vec<Field<'reg>>,
    values: alloc::vec::Vec<Option<alloc::vec::Vec<u8>>>,
    current: Option<usize>,
}

impl<'reg> NamedFields<'reg> {
    fn new(fields: alloc::vec::Vec<Field<'reg>>) -> Self {
        let values = fields.iter().map(|_| None).collect();
        NamedFields {
            fields,
            values,
            current: None,
        }
    }

    fn select<K: Serialize + ?Sized>(&mut self, key: &K) -> Result<()> {
        let key = to_vec(key)?;
        let index = self
            .fields
            .iter()
            .position(|field| to_vec(&field.name).is_ok_and(|name| name == key))
            .ok_or_else(|| Error::BadInput(format!("unknown field {}", key_text(&key))))?;
        if self.values[index].is_some() {
            return Err(Error::BadInput(format!(
                "duplicate field {}",
                self.fields[index].name
            )));
        }
        self.current = Some(index);
        Ok(())
    }

    fn encode<T: Serialize + ?Sized>(&mut self, registry: &'reg Registry, value: &T) -> Result<()> {
        let index = self
            .current
            .take()
            .ok_or_else(|| Error::BadInput("struct value without a field name".into()))?;
        let mut field = Serializer::new(
            alloc::vec::Vec::new(),
            Some((registry, self.fields[index].ty)),
        );
        value.serialize(&mut field)?;
        self.values[index] = Some(field.out);
        Ok(())
    }

    fn finish(self, registry: &Registry, out: &mut impl BufMut) -> Result<()> {
        for (field, value) in self.fields.iter().zip(self.values) {
            match value {
                Some(bytes) => out.put_slice(&bytes),
                None if is_option(registry, field.ty) => out.put_u8(0x00),
                None => {
                    return Err(Error::BadInput(format!("missing field {}", field.name)));
                }
            }
        }
        Ok(())
    }
}

fn is_option(registry: &Registry, ty: TypeId) -> bool {
    matches!(
        registry.resolve(unwrap_newtype(registry, ty)),
        Some(TypeDef::Variant(def)) if def.name() == "Option"
    )
}

/// A SCALE-encoded string key, for error messages.
fn key_text(key: &[u8]) -> alloc::string::String {
    let text = key
        .iter()
        .position(|&b| b.is_ascii_graphic())
        .map_or(key, |i| &key[i..]);
    alloc::string::String::from_utf8_lossy(text).into_owned()
}

#[derive(Debug)]
pub enum TypedSerializer<'a, 'reg, B>
where
    B: Debug,
{
    Empty(&'a mut Serializer<'reg, B>),
    Composite(&'a mut Serializer<'reg, B>, FieldTypes<'reg>),
    Named(&'a mut Serializer<'reg, B>, NamedFields<'reg>),
    Sequence(&'a mut Serializer<'reg, B>, TypeId),
    ByteSeq(&'a mut Serializer<'reg, B>),
    Enum(&'a mut Serializer<'reg, B>),
}

impl<'a, 'reg, B: 'a> From<&'a mut Serializer<'reg, B>> for TypedSerializer<'a, 'reg, B>
where
    B: Debug,
{
    fn from(ser: &'a mut Serializer<'reg, B>) -> Self {
        let ty_id = match ser.ty_id.take() {
            Some(id) => id,
            None => return Self::Empty(ser),
        };
        let ty = ser.resolve(ty_id);
        match ty {
            TypeDef::Struct(fields) => Self::Named(ser, NamedFields::new(fields.iter().collect())),
            TypeDef::StructTuple(ids) => Self::Composite(ser, FieldTypes::Ids(ids)),
            TypeDef::Array(ty, _) => Self::Sequence(ser, ty),
            TypeDef::Tuple(ids) => Self::Composite(ser, FieldTypes::Ids(ids)),
            TypeDef::Sequence(ty) => Self::Sequence(ser, ty),
            TypeDef::Bytes => Self::ByteSeq(ser),
            TypeDef::Map(_, _) => Self::Empty(ser),
            TypeDef::Variant(vdef) => {
                if let Some(idx) = ser.picked.take() {
                    let var = vdef.variants().nth(idx);
                    match var.map(|v| v.fields()) {
                        Some(Fields::Tuple(types)) => Self::Composite(ser, FieldTypes::Ids(types)),
                        Some(Fields::Struct(fields)) => {
                            Self::Named(ser, NamedFields::new(fields.iter().collect()))
                        }
                        _ => Self::Empty(ser),
                    }
                } else {
                    ser.ty_id = Some(ty_id);
                    Self::Enum(ser)
                }
            }
            _ => Self::Empty(ser),
        }
    }
}

impl<'reg, B> TypedSerializer<'_, 'reg, B>
where
    B: Debug,
{
    fn serializer(&mut self) -> &mut Serializer<'reg, B> {
        match self {
            Self::Empty(ser)
            | Self::Composite(ser, _)
            | Self::Named(ser, _)
            | Self::Enum(ser)
            | Self::Sequence(ser, _)
            | Self::ByteSeq(ser) => ser,
        }
    }
}

/// Resolve a field type, unwrapping newtypes (serde_json unwraps them).
///
/// Unwraps every layer: runtime types nest newtypes, e.g. a
/// `BoundedBTreeSet` around a `BTreeSet` around a sequence.
fn unwrap_newtype(reg: &Registry, mut ty_id: TypeId) -> TypeId {
    while let TypeDef::StructNewType(inner) = reg.resolve(ty_id).expect("in registry") {
        ty_id = inner;
    }
    ty_id
}

impl<B> ser::SerializeMap for TypedSerializer<'_, '_, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_key<T>(&mut self, key: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        match self {
            TypedSerializer::Enum(ser) => {
                if let Some(ty_id) = ser.ty_id {
                    if let TypeDef::Variant(vdef) = ser.resolve(ty_id) {
                        let key_data = to_vec(key)?;
                        let variants: alloc::vec::Vec<_> = vdef.variants().collect();
                        let idx = variants
                            .iter()
                            .position(|v| to_vec(&v.name()).is_ok_and(|d| d == key_data))
                            .ok_or_else(|| Error::BadInput("Invalid variant".into()))?;
                        let variant_index = variants[idx].index();
                        ser.picked = Some(idx);
                        variant_index.serialize(&mut **ser)?;
                    }
                }
                Ok(())
            }
            TypedSerializer::Empty(ser) => key.serialize(&mut **ser),
            TypedSerializer::Named(_, fields) => fields.select(key),
            _ => Ok(()),
        }
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        match self {
            TypedSerializer::Named(ser, fields) => {
                let registry = ser.registry.expect("called having type");
                return fields.encode(registry, value);
            }
            TypedSerializer::Composite(ser, field_types) => {
                let reg = ser.registry.expect("called having type");
                ser.ty_id = Some(unwrap_newtype(reg, field_types.next()));
                ser.ty_hint = None;
            }
            TypedSerializer::Enum(ser) => {
                if let Some(ty_id) = ser.ty_id {
                    if let TypeDef::Variant(vdef) = ser.resolve(ty_id) {
                        if let Some(idx) = ser.picked {
                            if let Some(var) = vdef.variants().nth(idx) {
                                if let Fields::NewType(field_ty_id) = var.fields() {
                                    let reg = ser.registry.expect("called having type");
                                    ser.ty_id = Some(unwrap_newtype(reg, field_ty_id));
                                    ser.ty_hint = None;
                                    ser.picked = None;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        finish(self)
    }
}

impl<B> ser::SerializeSeq for TypedSerializer<'_, '_, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        match self {
            TypedSerializer::Composite(ser, field_types) => {
                let reg = ser.registry.expect("called having type");
                ser.ty_id = Some(unwrap_newtype(reg, field_types.next()));
                ser.ty_hint = None;
            }
            TypedSerializer::Sequence(ser, ty_id) => {
                let reg = ser.registry.expect("called having type");
                ser.ty_id = Some(unwrap_newtype(reg, *ty_id));
                ser.ty_hint = None;
            }
            TypedSerializer::ByteSeq(ser) => {
                ser.ty_id = None;
                ser.ty_hint = Some(TypeDef::U8);
            }
            _ => {}
        };
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<B> ser::SerializeStruct for TypedSerializer<'_, '_, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        if let TypedSerializer::Named(ser, fields) = self {
            let registry = ser.registry.expect("called having type");
            fields.select(key)?;
            return fields.encode(registry, value);
        }
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        finish(self)
    }
}

/// Write buffered named fields, if any.
fn finish<B: BufMut + Debug>(serializer: TypedSerializer<'_, '_, B>) -> Result<()> {
    if let TypedSerializer::Named(ser, fields) = serializer {
        let registry = ser.registry.expect("called having type");
        fields.finish(registry, &mut ser.out)?;
    }
    Ok(())
}

impl<B> ser::SerializeStructVariant for TypedSerializer<'_, '_, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<B> ser::SerializeTuple for TypedSerializer<'_, '_, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<B> ser::SerializeTupleStruct for TypedSerializer<'_, '_, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<B> ser::SerializeTupleVariant for TypedSerializer<'_, '_, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

fn compact_number(n: usize, dest: impl BufMut) {
    crate::compact_encode(n as u128, dest)
}

// nightly only
fn type_name_of_val<T: ?Sized>(_val: &T) -> &'static str {
    core::any::type_name::<T>()
}
