use crate::error::Error;
use crate::prelude::*;
use crate::registry::*;
use bytes::BufMut;
use core::fmt::Debug;

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

fn compact_number(n: usize, dest: impl BufMut) {
    crate::compact_encode(n as u128, dest)
}

// nightly only
fn type_name_of_val<T: ?Sized>(_val: &T) -> &'static str {
    core::any::type_name::<T>()
}
