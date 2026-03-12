use crate::prelude::*;
use crate::registry::*;
use bytes::BufMut;
use core::fmt::{self, Debug};

use serde::{ser, Serialize};

type Result<T> = core::result::Result<T, Error>;

#[inline]
pub fn to_vec<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize + ?Sized,
{
    let mut out = vec![];
    to_bytes(&mut out, value)?;
    Ok(out)
}

#[inline]
pub fn to_vec_with_info<T>(value: &T, registry_type: Option<(&Registry, TypeId)>) -> Result<Vec<u8>>
where
    T: Serialize + ?Sized,
{
    let mut out = vec![];
    to_bytes_with_info(&mut out, value, registry_type)?;
    Ok(out)
}

pub fn to_bytes<B, T>(bytes: B, value: &T) -> Result<()>
where
    T: Serialize + ?Sized,
    B: BufMut + Debug,
{
    to_bytes_with_info(bytes, value, None)
}

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
    let val: crate::JsonValue = if let TypeDef::Struct(ref fields) = *ty {
        fields
            .iter()
            .map(|f| {
                Ok((
                    &*f.name,
                    obj.get(&f.name)
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
    ty: Option<TypeDef>,
    registry: Option<&'reg Registry>,
    picked: Option<usize>,
}

impl<'reg, B> Serializer<'reg, B>
where
    B: BufMut + Debug,
{
    pub fn new(out: B, registry_type: Option<(&'reg Registry, TypeId)>) -> Self {
        let (registry, ty) = match registry_type {
            Some((reg, ty_id)) => {
                let ty = reg.resolve(ty_id).expect("exists in registry").clone();
                (Some(reg), Some(ty))
            }
            None => (None, None),
        };
        Serializer {
            out,
            ty,
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
        match self.ty {
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
        match self.ty {
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
        match self.ty {
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
        self.out.put_u8(0x01);
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok> {
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
            self.ty,
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
        if matches!(self.ty, None | Some(TypeDef::Map(_, _))) {
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

impl<B> Serializer<'_, B>
where
    B: BufMut + Debug,
{
    // A check to run for every serialize fn since any type could be an Option::Some
    // if the type info says its an Option assume its Some and extract the inner type
    fn maybe_some(&mut self) -> Result<()> {
        match &self.ty {
            Some(TypeDef::Variant(ref vdef)) if vdef.name == "Option" => {
                self.ty = match &vdef.variants[1].fields {
                    Fields::NewType(ty_id) => Some(self.resolve(*ty_id)),
                    _ => None,
                };
                self.out.put_u8(0x01);
            }
            _ => (),
        }
        Ok(())
    }

    fn resolve(&self, ty_id: TypeId) -> TypeDef {
        let reg = self.registry.expect("called having type");
        reg.resolve(ty_id).expect("in registry").clone()
    }

    #[inline]
    fn maybe_other(&mut self, val: &str) -> Result<Option<()>> {
        match self.ty {
            Some(TypeDef::Str) | None => Ok(None),
            // { "foo": "Bar" } => "Bar" might be an enum variant
            Some(TypeDef::Variant(ref vdef)) => {
                let key_data = to_vec(val)?;
                let variant = vdef
                    .variants
                    .iter()
                    .find(|v| to_vec(&v.name).unwrap() == key_data)
                    .ok_or_else(|| Error::BadInput("Invalid variant".into()))?;
                self.out.put_u8(variant.index);
                Ok(Some(()))
            }
            Some(TypeDef::StructNewType(ty)) => match self.resolve(ty) {
                // { "foo": "bar" } => "bar" might be a string wrapped in a type
                TypeDef::Str => Ok(None),
                ref ty => Err(Error::NotSupported(
                    type_name_of_val(val),
                    format!("{:?}", ty),
                )),
            },
            Some(TypeDef::U8) => {
                let n = val.parse().map_err(|_| Error::BadInput("u8".into()))?;
                self.out.put_u8(n);
                Ok(Some(()))
            }
            Some(TypeDef::U16) => {
                let n = val.parse().map_err(|_| Error::BadInput("u16".into()))?;
                self.out.put_u16_le(n);
                Ok(Some(()))
            }
            Some(TypeDef::U32) => {
                let n = val.parse().map_err(|_| Error::BadInput("u32".into()))?;
                self.out.put_u32_le(n);
                Ok(Some(()))
            }
            Some(TypeDef::U64) => {
                let n = val.parse().map_err(|_| Error::BadInput("u64".into()))?;
                self.out.put_u64_le(n);
                Ok(Some(()))
            }
            Some(TypeDef::U128) => {
                let n = val.parse().map_err(|_| Error::BadInput("u128".into()))?;
                self.out.put_u128_le(n);
                Ok(Some(()))
            }
            Some(TypeDef::I8) => {
                let n = val.parse().map_err(|_| Error::BadInput("i8".into()))?;
                self.out.put_i8(n);
                Ok(Some(()))
            }
            Some(TypeDef::I16) => {
                let n = val.parse().map_err(|_| Error::BadInput("i16".into()))?;
                self.out.put_i16_le(n);
                Ok(Some(()))
            }
            Some(TypeDef::I32) => {
                let n = val.parse().map_err(|_| Error::BadInput("i32".into()))?;
                self.out.put_i32_le(n);
                Ok(Some(()))
            }
            Some(TypeDef::I64) => {
                let n = val.parse().map_err(|_| Error::BadInput("i64".into()))?;
                self.out.put_i64_le(n);
                Ok(Some(()))
            }
            Some(TypeDef::I128) => {
                let n = val.parse().map_err(|_| Error::BadInput("i128".into()))?;
                self.out.put_i128_le(n);
                Ok(Some(()))
            }
            #[cfg(feature = "hex")]
            Some(TypeDef::Bytes) => {
                if let Some(bytes) = val.strip_prefix("0x") {
                    let bytes = hex::decode(bytes).map_err(|e| Error::BadInput(e.to_string()))?;
                    ser::Serializer::serialize_bytes(self, &bytes)?;
                    Ok(Some(()))
                } else {
                    Err(Error::BadInput("Hex string must start with 0x".into()))
                }
            }
            Some(ref ty) => Err(Error::NotSupported(
                type_name_of_val(val),
                format!("{:?}", ty),
            )),
        }
    }
}

#[derive(Debug)]
pub enum TypedSerializer<'a, 'reg, B>
where
    B: Debug,
{
    Empty(&'a mut Serializer<'reg, B>),
    Composite(&'a mut Serializer<'reg, B>, Vec<TypeId>),
    Sequence(&'a mut Serializer<'reg, B>, TypeId),
    ByteSeq(&'a mut Serializer<'reg, B>),
    Enum(&'a mut Serializer<'reg, B>),
}

impl<'a, 'reg, B: 'a> From<&'a mut Serializer<'reg, B>> for TypedSerializer<'a, 'reg, B>
where
    B: Debug,
{
    fn from(ser: &'a mut Serializer<'reg, B>) -> Self {
        match ser.ty.take() {
            Some(TypeDef::Struct(fields)) => {
                Self::Composite(ser, fields.iter().map(|f| f.ty).collect())
            }
            Some(TypeDef::StructTuple(fields)) => Self::Composite(ser, fields),
            Some(TypeDef::Array(ty, _)) => Self::Sequence(ser, ty),
            Some(TypeDef::Tuple(fields)) => Self::Composite(ser, fields),
            Some(TypeDef::Sequence(ty)) => Self::Sequence(ser, ty),
            Some(TypeDef::Bytes) => Self::ByteSeq(ser),
            Some(TypeDef::Map(_, _)) => Self::Empty(ser),
            Some(TypeDef::Variant(vdef)) => {
                if let Some(idx) = ser.picked.take() {
                    match &vdef.variants[idx].fields {
                        Fields::Tuple(types) => Self::Composite(ser, types.clone()),
                        Fields::Struct(fields) => {
                            Self::Composite(ser, fields.iter().map(|f| f.ty).collect())
                        }
                        _ => Self::Empty(ser),
                    }
                } else {
                    ser.ty = Some(TypeDef::Variant(vdef));
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
            | Self::Enum(ser)
            | Self::Sequence(ser, _)
            | Self::ByteSeq(ser) => ser,
        }
    }
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
                if let Some(TypeDef::Variant(ref vdef)) = ser.ty {
                    let key_data = to_vec(key)?;
                    let idx = vdef
                        .variants
                        .iter()
                        .position(|v| to_vec(&v.name).unwrap() == key_data)
                        .ok_or_else(|| Error::BadInput("Invalid variant".into()))?;
                    let variant_index = vdef.variants[idx].index;
                    ser.picked = Some(idx);
                    variant_index.serialize(&mut **ser)?;
                }
                Ok(())
            }
            TypedSerializer::Empty(ser) => key.serialize(&mut **ser),
            _ => Ok(()),
        }
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        match self {
            TypedSerializer::Composite(ser, types) => {
                let mut ty = ser.resolve(types.remove(0));
                // serde_json unwraps newtypes
                if let TypeDef::StructNewType(ty_id) = ty {
                    ty = ser.resolve(ty_id)
                }
                ser.ty = Some(ty);
            }
            TypedSerializer::Enum(ser) => {
                if let Some(TypeDef::Variant(ref vdef)) = ser.ty {
                    if let Some(idx) = ser.picked {
                        if let Fields::NewType(ty_id) = &vdef.variants[idx].fields {
                            let ty = ser.resolve(*ty_id);
                            ser.ty = Some(if let TypeDef::StructNewType(inner) = ty {
                                ser.resolve(inner)
                            } else {
                                ty
                            });
                            ser.picked = None;
                        }
                    }
                }
            }
            _ => {}
        }
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
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
            TypedSerializer::Composite(ser, types) => {
                let mut ty = ser.resolve(types.remove(0));
                if let TypeDef::StructNewType(ty_id) = ty {
                    ty = ser.resolve(ty_id);
                }
                ser.ty = Some(ty);
            }
            TypedSerializer::Sequence(ser, ty_id) => {
                let ty = ser.resolve(*ty_id);
                ser.ty = Some(match ty {
                    TypeDef::StructNewType(ty_id) => ser.resolve(ty_id),
                    _ => ty,
                });
            }
            TypedSerializer::ByteSeq(ser) => {
                ser.ty = Some(TypeDef::U8);
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

#[derive(Debug)]
pub enum Error {
    Ser(String),
    BadInput(String),
    BadType(String),
    NotSupported(&'static str, String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Ser(msg) => write!(f, "{}", msg),
            Error::BadInput(msg) => write!(f, "Bad Input: {}", msg),
            Error::BadType(msg) => write!(f, "Unexpected type: {}", msg),
            Error::NotSupported(from, to) => {
                write!(f, "Serializing {} as {} is not supported", from, to)
            }
        }
    }
}

impl core::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T>(msg: T) -> Self
    where
        T: fmt::Display,
    {
        Error::Ser(msg.to_string())
    }
}

fn compact_number(n: usize, dest: impl BufMut) {
    crate::compact_encode(n as u128, dest)
}

// nightly only
fn type_name_of_val<T: ?Sized>(_val: &T) -> &'static str {
    core::any::type_name::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use codec::{Decode, Encode};
    use core::mem::size_of;
    use scale_info::{meta_type, PortableRegistry, Registry as SiRegistry, TypeInfo};
    use serde_json::to_value;

    #[test]
    fn primitive_u8() -> Result<()> {
        let mut out = [0u8];
        to_bytes(&mut out[..], &123u8)?;

        let expected = [123];

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn primitive_u16() -> Result<()> {
        const INPUT: u16 = 0xFF_EE;
        let mut out = [0u8; size_of::<u16>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_ref(), expected);
        Ok(())
    }

    #[test]
    fn primitive_u32() -> Result<()> {
        const INPUT: u32 = 0xFF_EE_DD_CC;
        let mut out = [0u8; size_of::<u32>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_ref(), expected);
        Ok(())
    }

    #[test]
    fn primitive_u64() -> Result<()> {
        const INPUT: u64 = 0xFFEE_DDCC_BBAA_9988;
        let mut out = [0u8; size_of::<u64>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn primitive_u128() -> Result<()> {
        const INPUT: u128 = 0xFFEE_DDCC_BBAA_9988_7766_5544_3322_1100;
        let mut out = [0u8; size_of::<u128>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn primitive_i16() -> Result<()> {
        const INPUT: i16 = i16::MIN;
        let mut out = [0u8; size_of::<i16>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn primitive_i32() -> Result<()> {
        const INPUT: i32 = i32::MIN;
        let mut out = [0u8; size_of::<i32>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn primitive_i64() -> Result<()> {
        const INPUT: i64 = i64::MIN;
        let mut out = [0u8; size_of::<i64>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn primitive_bool() -> Result<()> {
        const INPUT: bool = true;
        let mut out = [0u8];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn str() -> Result<()> {
        const INPUT: &str = "ac orci phasellus egestas tellus rutrum tellus pellentesque";
        let mut out = Vec::<u8>::new();
        let expected = INPUT.encode();

        to_bytes(&mut out, &INPUT)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn bytes() -> Result<()> {
        const INPUT: &[u8] = b"dictumst quisque sagittis purus sit amet volutpat consequat";
        let mut out = Vec::<u8>::new();
        let expected = INPUT.encode();

        to_bytes(&mut out, &INPUT)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn tuple_simple() -> Result<()> {
        const INPUT: (u8, bool, u64) = (0xD0, false, u64::MAX);
        let mut out = Vec::<u8>::new();
        let expected = INPUT.encode();

        to_bytes(&mut out, &INPUT)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn enum_simple() -> Result<()> {
        #[derive(Serialize, Encode)]
        enum X {
            _A,
            B,
        }

        const INPUT: X = X::B;
        let mut out = Vec::<u8>::new();
        let expected = INPUT.encode();

        to_bytes(&mut out, &INPUT)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn tuple_enum_mix() -> Result<()> {
        #[derive(Serialize, Encode)]
        enum X {
            A,
            B,
        }

        let input: (Option<()>, Option<String>, X, X) = (None, Some("hello".into()), X::A, X::B);
        let mut out = Vec::<u8>::new();
        let expected = input.encode();

        to_bytes(&mut out, &input)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn struct_simple() -> Result<()> {
        #[derive(Serialize, Encode)]
        struct Foo {
            a: Bar,
            b: Option<Baz>,
        }
        #[derive(Serialize, Encode)]
        struct Bar(u8);
        #[derive(Serialize, Encode)]
        struct Baz(String, u16);

        let input = Foo {
            a: Bar(0xFF),
            b: Some(Baz("lol".into(), u16::MAX)),
        };
        let mut out = Vec::<u8>::new();
        let expected = input.encode();

        to_bytes(&mut out, &input)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn vec_simple() -> Result<()> {
        let input: Vec<String> = vec!["hello".into(), "beautiful".into(), "people".into()];
        let mut out = Vec::<u8>::new();
        let expected = input.encode();

        to_bytes(&mut out, &input)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn struct_mix() -> Result<()> {
        #[derive(Serialize, Encode)]
        struct Foo<'a> {
            a: Vec<String>,
            b: (Bar<'a>, Bar<'a>, Bar<'a>),
        }
        #[derive(Serialize, Encode)]
        enum Bar<'a> {
            A { thing: &'a str },
            B(Baz),
            C(BTreeMap<String, bool>, i64),
        }
        #[derive(Serialize, Encode)]
        struct Baz;

        let input = Foo {
            a: vec!["hello".into(), "beautiful".into(), "people".into()],
            b: (
                Bar::A { thing: "barbarbar" },
                Bar::B(Baz),
                Bar::C(
                    {
                        let mut h = BTreeMap::new();
                        h.insert("key".into(), false);
                        h
                    },
                    i64::MIN,
                ),
            ),
        };
        let mut out = Vec::<u8>::new();
        let expected = input.encode();

        to_bytes(&mut out, &input)?;

        assert_eq!(out, expected);
        Ok(())
    }

    fn register<T>(_ty: &T) -> (TypeId, crate::Registry)
    where
        T: TypeInfo + 'static,
    {
        let mut reg = SiRegistry::new();
        let sym = reg.register_type(&meta_type::<T>());
        let portable: PortableRegistry = reg.into();
        (sym.id, crate::compress::compress(&portable))
    }

    #[test]
    fn str_as_u128() -> Result<()> {
        const INPUT: &str = "340282366920938463463374607431768211455";
        let mut out = [0u8; size_of::<u128>()];
        let expected = u128::MAX.encode();

        let (id, reg) = register(&0u128);

        to_bytes_with_info(out.as_mut(), &INPUT, Some((&reg, id)))?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn json_simple() -> Result<()> {
        #[derive(Debug, Serialize, Encode, TypeInfo)]
        struct Foo {
            a: Bar,
            b: Option<Baz>,
        }
        #[derive(Debug, Serialize, Encode, TypeInfo)]
        struct Bar(u8);
        #[derive(Debug, Serialize, Encode, TypeInfo)]
        struct Baz(String, i32);

        let input = Foo {
            a: Bar(0xFF),
            b: Some(Baz("lol".into(), i32::MIN)),
        };
        let mut out = Vec::<u8>::new();
        let expected = input.encode();
        let (id, reg) = register(&input);

        let json_input = to_value(&input).unwrap();
        to_bytes_with_info(&mut out, &json_input, Some((&reg, id)))?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn json_mix() -> Result<()> {
        #[derive(Debug, Serialize, Encode, TypeInfo)]
        struct Foo {
            a: Vec<String>,
            b: (Bar, Bar, Bar),
        }
        #[derive(Debug, Serialize, Encode, TypeInfo)]
        enum Bar {
            A { thing: &'static str },
            B(Baz),
            C(BTreeMap<String, bool>, i64),
        }
        #[derive(Debug, Serialize, Encode, TypeInfo)]
        struct Baz;

        let input = Foo {
            a: vec!["hello".into(), "beautiful".into(), "people".into()],
            b: (
                Bar::A { thing: "barbarbar" },
                Bar::B(Baz),
                Bar::C(
                    {
                        let mut h = BTreeMap::new();
                        h.insert("key1".into(), false);
                        h.insert("key2".into(), true);
                        h
                    },
                    i64::MIN,
                ),
            ),
        };
        let mut out = Vec::<u8>::new();
        let expected = input.encode();
        let (id, reg) = register(&input);

        let json_input = to_value(&input).unwrap();
        to_bytes_with_info(&mut out, &json_input, Some((&reg, id)))?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn json_mix2() -> Result<()> {
        #[derive(Debug, Encode, Serialize, TypeInfo)]
        enum Bar {
            This,
            That(i16),
        }
        #[derive(Debug, Encode, Serialize, TypeInfo)]
        struct Baz(String);
        #[derive(Debug, Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: Vec<Bar>,
            baz: Option<Baz>,
            lol: &'static [u8],
        }
        let input = Foo {
            bar: [Bar::That(i16::MAX), Bar::This].into(),
            baz: Some(Baz("lorem ipsum".into())),
            lol: b"\xFFsome stuff\x00",
        };
        let mut out = Vec::<u8>::new();
        let expected = input.encode();
        let (id, reg) = register(&input);

        let json_input = to_value(&input).unwrap();
        to_bytes_with_info(&mut out, &json_input, Some((&reg, id)))?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_unordered_iter() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        enum Bar {
            _This,
            That(i16),
        }
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            bar: Bar,
            baz: Option<u32>,
            bam: String,
        }
        let foo = Foo {
            bar: Bar::That(i16::MAX),
            baz: Some(123),
            bam: "lorem ipsum".into(),
        };
        let (ty, reg) = register(&foo);

        let input = vec![
            ("bam", crate::JsonValue::String("lol".into())),
            ("baz", 123.into()),
            ("bam", "lorem ipsum".into()),
            ("bar", serde_json::json!({ "That": i16::MAX })),
        ];

        let out = to_vec_from_iter(input, (&reg, ty))?;
        let expected = foo.encode();

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_bytes_as_hex_string() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            bar: Vec<u8>,
        }
        let foo = Foo {
            bar: b"\x00\x12\x34\x56".to_vec(),
        };
        let (ty, reg) = register(&foo);

        let hex_string = "0x00123456";

        let input = vec![("bar", crate::JsonValue::String(hex_string.into()))];

        let out = to_vec_from_iter(input, (&reg, ty))?;
        let expected = foo.encode();

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_extrincic_call() -> Result<()> {
        let bytes = include_bytes!("registry.bin");
        let portable = PortableRegistry::decode(&mut &bytes[..]).expect("hello");
        let registry = crate::compress::compress(&portable);

        let transfer_call = serde_json::json!({
            "transfer_keep_alive": {
                "dest": {
                    "Id": hex::decode("12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b").expect("expected valid address")
                },
                "value": 1_000_000_000_000u64
            }
        });

        let call_data =
            to_vec_with_info(&transfer_call, (&registry, 106u32).into()).expect("call data");

        let encooded = hex::encode(call_data);

        assert_eq!(
            "0x04030012840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b070010a5d4e8",
            format!("0x04{}", encooded)
        );

        Ok(())
    }

    // --- Type coercion tests ---

    #[test]
    fn json_u64_coerced_to_u8() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            val: u8,
        }
        let foo = Foo { val: 42 };
        let (id, reg) = register(&foo);
        let json = serde_json::to_value(&foo).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, foo.encode());
        Ok(())
    }

    #[test]
    fn json_u64_coerced_to_u16() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            val: u16,
        }
        let foo = Foo { val: 1000 };
        let (id, reg) = register(&foo);
        let json = serde_json::to_value(&foo).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, foo.encode());
        Ok(())
    }

    #[test]
    fn json_u64_coerced_to_u32() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            val: u32,
        }
        let foo = Foo { val: 100_000 };
        let (id, reg) = register(&foo);
        let json = serde_json::to_value(&foo).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, foo.encode());
        Ok(())
    }

    #[test]
    fn json_u64_coerced_to_i32() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            val: i32,
        }
        let foo = Foo { val: -1 };
        let (id, reg) = register(&foo);
        let json = serde_json::to_value(&foo).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, foo.encode());
        Ok(())
    }

    #[test]
    fn json_string_coerced_to_u8() -> Result<()> {
        let (id, reg) = register(&0u8);
        let mut out = Vec::new();
        to_bytes_with_info(&mut out, &"255", Some((&reg, id)))?;
        assert_eq!(out, [255u8]);
        Ok(())
    }

    #[test]
    fn json_string_coerced_to_i64() -> Result<()> {
        let (id, reg) = register(&0i64);
        let mut out = Vec::new();
        to_bytes_with_info(&mut out, &"-9223372036854775808", Some((&reg, id)))?;
        assert_eq!(out, i64::MIN.encode());
        Ok(())
    }

    #[test]
    fn json_compact_u32() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            #[codec(compact)]
            val: u32,
        }
        let foo = Foo { val: 69 };
        let (id, reg) = register(&foo);
        let json = serde_json::to_value(&foo).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, foo.encode());
        Ok(())
    }

    // --- Option handling ---

    #[test]
    fn json_option_some_nested() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            val: Option<u32>,
        }
        let foo = Foo { val: Some(42) };
        let (id, reg) = register(&foo);
        let json = serde_json::to_value(&foo).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, foo.encode());
        Ok(())
    }

    // NOTE: json_option_none is not tested because serde_json serializes
    // null via serialize_unit(), not serialize_none(), so the type-info
    // path treats it as Some(unit). This is a known JSON→SCALE limitation.

    // --- Error paths ---

    #[test]
    fn invalid_variant_name() {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        enum Bar {
            A,
            #[allow(dead_code)]
            B,
        }
        let (id, reg) = register(&Bar::A);
        let json = serde_json::json!("NonExistent");

        let result = to_vec_with_info(&json, Some((&reg, id)));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_numeric_string() {
        let (id, reg) = register(&0u32);
        let result = to_vec_with_info(&"not_a_number", Some((&reg, id)));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_hex_string() {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            bar: Vec<u8>,
        }
        let foo = Foo { bar: vec![] };
        let (ty, reg) = register(&foo);

        // Missing 0x prefix
        let input = vec![("bar", crate::JsonValue::String("00123456".into()))];
        let result = to_vec_from_iter(input, (&reg, ty));
        assert!(result.is_err());

        // Invalid hex chars
        let input = vec![("bar", crate::JsonValue::String("0xGGHH".into()))];
        let result = to_vec_from_iter(input, (&reg, ty));
        assert!(result.is_err());
    }

    #[test]
    fn from_iter_missing_field() {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            bar: u32,
            baz: String,
        }
        let foo = Foo {
            bar: 1,
            baz: "x".into(),
        };
        let (ty, reg) = register(&foo);

        // Only provide one field
        let input = vec![("bar", crate::JsonValue::from(1))];
        let result = to_vec_from_iter(input, (&reg, ty));
        assert!(result.is_err());
    }

    // --- Nested struct variants ---

    #[test]
    fn json_struct_variant() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        enum Msg {
            #[allow(dead_code)]
            Ping,
            Data {
                id: u32,
                payload: String,
            },
        }
        let input = Msg::Data {
            id: 42,
            payload: "hello".into(),
        };
        let (id, reg) = register(&input);
        let json = serde_json::to_value(&input).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, input.encode());
        Ok(())
    }

    #[test]
    fn json_tuple_variant() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        enum Msg {
            #[allow(dead_code)]
            Ping,
            Pair(u32, u32),
        }
        let input = Msg::Pair(1, 2);
        let (id, reg) = register(&input);
        let json = serde_json::to_value(&input).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, input.encode());
        Ok(())
    }

    #[test]
    fn json_array_type() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            data: [u8; 4],
        }
        let foo = Foo { data: [1, 2, 3, 4] };
        let (id, reg) = register(&foo);
        let json = serde_json::to_value(&foo).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, foo.encode());
        Ok(())
    }

    #[test]
    fn json_btreemap() -> Result<()> {
        // NOTE: Map values aren't type-coerced (TypedSerializer::Empty for Map),
        // so we use bool values which don't need coercion (same size in JSON and SCALE)
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        struct Foo {
            map: BTreeMap<String, bool>,
        }
        let foo = Foo {
            map: {
                let mut m = BTreeMap::new();
                m.insert("a".into(), true);
                m.insert("b".into(), false);
                m
            },
        };
        let (id, reg) = register(&foo);
        let json = serde_json::to_value(&foo).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, foo.encode());
        Ok(())
    }

    #[test]
    fn json_nested_option_in_enum() -> Result<()> {
        #[derive(Debug, Encode, TypeInfo, Serialize)]
        enum Outer {
            #[allow(dead_code)]
            None,
            Some(Option<u32>),
        }
        let input = Outer::Some(Some(99));
        let (id, reg) = register(&input);
        let json = serde_json::to_value(&input).unwrap();

        let out = to_vec_with_info(&json, Some((&reg, id)))?;
        assert_eq!(out, input.encode());
        Ok(())
    }
}
