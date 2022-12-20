use crate::prelude::*;
use bytes::BufMut;
use codec::Encode;
use core::fmt::{self, Debug};
use scale_info::{PortableRegistry, TypeInfo};
use serde::{ser, Serialize};

use crate::{EnumVariant, SpecificType, TupleOrArray};

type TypeId = u32;
type Result<T> = core::result::Result<T, Error>;

#[derive(TypeInfo)]
struct Noop;

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
pub fn to_vec_with_info<T>(
    value: &T,
    registry_type: Option<(&PortableRegistry, TypeId)>,
) -> Result<Vec<u8>>
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
    registry_type: Option<(&PortableRegistry, TypeId)>,
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
pub fn to_bytes_from_iter<B, I, K, V>(
    bytes: B,
    iter: I,
    registry_type: (&PortableRegistry, TypeId),
) -> Result<()>
where
    B: BufMut + Debug,
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<crate::JsonValue>,
{
    let ty = registry_type
        .0
        .resolve(registry_type.1)
        .ok_or_else(|| Error::BadInput("Type not in registry".into()))?;
    let obj = iter.into_iter().collect::<crate::JsonValue>();
    let val: crate::JsonValue = if let scale_info::TypeDef::Composite(ty) = ty.type_def() {
        ty.fields()
            .iter()
            .map(|f| {
                let name = f.name().expect("named field");
                Ok((
                    name.deref(),
                    obj.get(name)
                        .ok_or_else(|| Error::BadInput(format!("missing field {}", name)))?
                        .clone(),
                ))
            })
            .collect::<Result<_>>()?
    } else {
        return Err(Error::Type(ty.clone()));
    };

    to_bytes_with_info(bytes, &val, Some(registry_type))
}

#[cfg(feature = "json")]
pub fn to_vec_from_iter<I, K, V>(
    iter: I,
    registry_type: (&PortableRegistry, TypeId),
) -> Result<Vec<u8>>
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
    ty: Option<SpecificType>,
    registry: Option<&'reg PortableRegistry>,
}

impl<'reg, B> Serializer<'reg, B>
where
    B: BufMut + Debug,
{
    pub fn new(out: B, registry_type: Option<(&'reg PortableRegistry, TypeId)>) -> Self {
        let (registry, ty) = match registry_type.map(|(reg, ty_id)| {
            (
                reg,
                (reg.resolve(ty_id).expect("exists in registry"), reg).into(),
            )
        }) {
            Some((reg, ty)) => (Some(reg), Some(ty)),
            None => (None, None),
        };
        Serializer { out, ty, registry }
    }

    fn serialize_compact(&mut self, ty: u32, v: u128) -> Result<()> {
        let type_def = self.resolve(ty);

        use codec::Compact;
        let compact_buffer = match type_def {
            SpecificType::U32 => Compact(v as u32).encode(),
            SpecificType::U64 => Compact(v as u64).encode(),
            SpecificType::U128 => Compact(v).encode(),
            _ => todo!(),
        };

        self.out.put_slice(&compact_buffer[..]);

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
            Some(SpecificType::I8) => self.serialize_i8(v as i8)?,
            Some(SpecificType::I16) => self.serialize_i16(v as i16)?,
            Some(SpecificType::I32) => self.serialize_i32(v as i32)?,
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
            Some(SpecificType::I8) => self.serialize_i8(v as i8)?,
            Some(SpecificType::I16) => self.serialize_i16(v as i16)?,
            Some(SpecificType::I32) => self.serialize_i32(v as i32)?,
            Some(SpecificType::U8) => self.serialize_u8(v as u8)?,
            Some(SpecificType::U16) => self.serialize_u16(v as u16)?,
            Some(SpecificType::U32) => self.serialize_u32(v as u32)?,
            Some(SpecificType::Compact(ty)) => self.serialize_compact(ty, v as u128)?,
            _ => self.out.put_u64_le(v),
        }
        Ok(())
    }

    fn serialize_u128(self, v: u128) -> Result<Self::Ok> {
        self.maybe_some()?;
        match self.ty {
            Some(SpecificType::I8) => self.serialize_i8(v as i8)?,
            Some(SpecificType::I16) => self.serialize_i16(v as i16)?,
            Some(SpecificType::I32) => self.serialize_i32(v as i32)?,
            Some(SpecificType::I64) => self.serialize_i64(v as i64)?,
            Some(SpecificType::U8) => self.serialize_u8(v as u8)?,
            Some(SpecificType::U16) => self.serialize_u16(v as u16)?,
            Some(SpecificType::U32) => self.serialize_u32(v as u32)?,
            Some(SpecificType::U64) => self.serialize_u64(v as u64)?,
            Some(SpecificType::Compact(ty)) => self.serialize_compact(ty, v as u128)?,
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

    fn serialize_some<T: ?Sized>(self, value: &T) -> Result<Self::Ok>
    where
        T: Serialize,
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

    fn serialize_newtype_struct<T: ?Sized>(self, _name: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: Serialize,
    {
        self.maybe_some()?;
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized>(
        self,
        __name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok>
    where
        T: Serialize,
    {
        self.maybe_some()?;
        self.out.put_u8(variant_index as u8);
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        self.maybe_some()?;
        if matches!(
            self.ty,
            None | Some(SpecificType::Bytes(_)) | Some(SpecificType::Sequence(_))
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
        if matches!(self.ty, None | Some(SpecificType::Map(_, _))) {
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

impl<'a, 'reg, B> Serializer<'reg, B>
where
    B: BufMut + Debug,
{
    // A check to run for every serialize fn since any type could be an Option::Some
    // if the type info says its an Option assume its Some and extract the inner type
    fn maybe_some(&mut self) -> Result<()> {
        match &self.ty {
            Some(SpecificType::Variant(ref name, v, _)) if name == "Option" => {
                self.ty = v[1].fields().first().map(|f| self.resolve(f.ty().id()));
                self.out.put_u8(0x01);
            }
            _ => (),
        }
        Ok(())
    }

    fn resolve(&self, ty_id: TypeId) -> SpecificType {
        let reg = self.registry.expect("called having type");
        let ty = reg.resolve(ty_id).expect("in registry");
        (ty, reg).into()
    }

    #[inline]
    fn maybe_other(&mut self, val: &str) -> Result<Option<()>> {
        match self.ty {
            Some(SpecificType::Str) | None => Ok(None),
            // { "foo": "Bar" } => "Bar" might be an enum variant
            Some(ref mut var @ SpecificType::Variant(_, _, None)) => {
                var.pick_mut(to_vec(val)?, |k| to_vec(k.name()).unwrap())
                    .ok_or_else(|| Error::BadInput("Invalid variant".into()))?;
                self.out.put_u8(var.variant_id());
                Ok(Some(()))
            }
            Some(SpecificType::StructNewType(ty)) => match self.resolve(ty) {
                // { "foo": "bar" } => "bar" might be a string wrapped in a type
                SpecificType::Str => Ok(None),
                ref ty => Err(Error::NotSupported(
                    type_name_of_val(val),
                    format!("{:?}", ty),
                )),
            },
            Some(SpecificType::U8) => {
                let n = val.parse().map_err(|_| Error::BadInput("u8".into()))?;
                Ok(Some(self.out.put_u8(n)))
            }
            Some(SpecificType::U16) => {
                let n = val.parse().map_err(|_| Error::BadInput("u16".into()))?;
                Ok(Some(self.out.put_u16_le(n)))
            }
            Some(SpecificType::U32) => {
                let n = val.parse().map_err(|_| Error::BadInput("u32".into()))?;
                Ok(Some(self.out.put_u32_le(n)))
            }
            Some(SpecificType::U64) => {
                let n = val.parse().map_err(|_| Error::BadInput("u64".into()))?;
                Ok(Some(self.out.put_u64_le(n)))
            }
            Some(SpecificType::U128) => {
                let n = val.parse().map_err(|_| Error::BadInput("u128".into()))?;
                Ok(Some(self.out.put_u128_le(n)))
            }
            Some(SpecificType::I8) => {
                let n = val.parse().map_err(|_| Error::BadInput("i8".into()))?;
                Ok(Some(self.out.put_i8(n)))
            }
            Some(SpecificType::I16) => {
                let n = val.parse().map_err(|_| Error::BadInput("i16".into()))?;
                Ok(Some(self.out.put_i16_le(n)))
            }
            Some(SpecificType::I32) => {
                let n = val.parse().map_err(|_| Error::BadInput("i32".into()))?;
                Ok(Some(self.out.put_i32_le(n)))
            }
            Some(SpecificType::I64) => {
                let n = val.parse().map_err(|_| Error::BadInput("i64".into()))?;
                Ok(Some(self.out.put_i64_le(n)))
            }
            Some(SpecificType::I128) => {
                let n = val.parse().map_err(|_| Error::BadInput("i128".into()))?;
                Ok(Some(self.out.put_i128_le(n)))
            }
            #[cfg(feature = "hex")]
            Some(SpecificType::Bytes(_)) => {
                if val.starts_with("0x") {
                    let bytes =
                        hex::decode(&val[2..]).map_err(|e| Error::BadInput(e.to_string()))?;
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

///
#[derive(Debug)]
pub enum TypedSerializer<'a, 'reg, B>
where
    B: Debug,
{
    Empty(&'a mut Serializer<'reg, B>),
    Composite(&'a mut Serializer<'reg, B>, Vec<TypeId>),
    Sequence(&'a mut Serializer<'reg, B>, TypeId),
    Enum(&'a mut Serializer<'reg, B>),
}

impl<'a, 'reg, B: 'a> From<&'a mut Serializer<'reg, B>> for TypedSerializer<'a, 'reg, B>
where
    B: Debug,
{
    fn from(ser: &'a mut Serializer<'reg, B>) -> Self {
        use SpecificType::*;
        match ser.ty.take() {
            Some(Struct(fields)) => {
                Self::Composite(ser, fields.iter().map(|(_, ty)| *ty).collect())
            }
            Some(StructTuple(fields)) => Self::Composite(ser, fields),
            Some(Tuple(TupleOrArray::Array(ty, _))) => Self::Sequence(ser, ty),
            Some(Tuple(TupleOrArray::Tuple(fields))) => Self::Composite(ser, fields),
            Some(Sequence(ty) | Bytes(ty)) => Self::Sequence(ser, ty),
            Some(Map(_, _)) => Self::Empty(ser),
            Some(var @ Variant(_, _, Some(_))) => match (&var).into() {
                EnumVariant::Tuple(_, _, types) => Self::Composite(ser, types),
                EnumVariant::Struct(_, _, types) => {
                    Self::Composite(ser, types.iter().map(|(_, ty)| *ty).collect())
                }
                _ => Self::Empty(ser),
            },
            Some(var @ Variant(_, _, None)) => {
                ser.ty = Some(var);
                Self::Enum(ser)
            }
            _ => Self::Empty(ser),
        }
    }
}

impl<'a, 'reg, B> TypedSerializer<'a, 'reg, B>
where
    B: Debug,
{
    fn serializer(&mut self) -> &mut Serializer<'reg, B> {
        match self {
            Self::Empty(ser)
            | Self::Composite(ser, _)
            | Self::Enum(ser)
            | Self::Sequence(ser, _) => ser,
        }
    }
}

impl<'a, 'reg, B> ser::SerializeMap for TypedSerializer<'a, 'reg, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: ?Sized>(&mut self, key: &T) -> Result<()>
    where
        T: Serialize,
    {
        match self {
            TypedSerializer::Enum(ser) => {
                if let Some(ref mut var @ SpecificType::Variant(_, _, None)) = ser.ty {
                    let key_data = to_vec(key)?;
                    // assume the key is the name of the variant
                    var.pick_mut(key_data, |v| to_vec(v.name()).unwrap())
                        .ok_or_else(|| Error::BadInput("Invalid variant".into()))?
                        .variant_id()
                        .serialize(&mut **ser)?;
                }
                Ok(())
            }
            TypedSerializer::Empty(ser) => key.serialize(&mut **ser),
            _ => Ok(()),
        }
    }

    fn serialize_value<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        match self {
            TypedSerializer::Composite(ser, types) => {
                let mut ty = ser.resolve(types.remove(0));
                // serde_json unwraps newtypes
                if let SpecificType::StructNewType(ty_id) = ty {
                    ty = ser.resolve(ty_id)
                }
                ser.ty = Some(ty);
            }
            TypedSerializer::Enum(ser) => {
                if let Some(var @ SpecificType::Variant(_, _, Some(_))) = &ser.ty {
                    if let EnumVariant::NewType(_, _, ty_id) = var.into() {
                        let ty = ser.resolve(ty_id);

                        ser.ty = Some(if let SpecificType::StructNewType(ty_id) = ty {
                            let ty = ser.resolve(ty_id);
                            ty
                        } else {
                            ty
                        });
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

impl<'a, 'reg, B> ser::SerializeSeq for TypedSerializer<'a, 'reg, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        match self {
            TypedSerializer::Composite(ser, types) => {
                let mut ty = ser.resolve(types.remove(0));
                if let SpecificType::StructNewType(ty_id) = ty {
                    ty = ser.resolve(ty_id);
                }
                ser.ty = Some(ty);
            }
            TypedSerializer::Sequence(ser, ty_id) => {
                let ty = ser.resolve(*ty_id);
                ser.ty = Some(match ty {
                    SpecificType::StructNewType(ty_id) => ser.resolve(ty_id),
                    _ => ty,
                });
            }
            _ => {}
        };
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, 'reg, B> ser::SerializeStruct for TypedSerializer<'a, 'reg, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized>(&mut self, _key: &'static str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, 'reg, B> ser::SerializeStructVariant for TypedSerializer<'a, 'reg, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized>(&mut self, _key: &'static str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, 'reg, B> ser::SerializeTuple for TypedSerializer<'a, 'reg, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, 'reg, B> ser::SerializeTupleStruct for TypedSerializer<'a, 'reg, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, 'reg, B> ser::SerializeTupleVariant for TypedSerializer<'a, 'reg, B>
where
    B: BufMut + Debug,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
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
    Type(scale_info::Type<scale_info::form::PortableForm>),
    NotSupported(&'static str, String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Ser(msg) => write!(f, "{}", msg),
            Error::BadInput(msg) => write!(f, "Bad Input: {}", msg),
            Error::Type(ty) => write!(
                f,
                "Unexpected type: {}",
                ty.path().ident().unwrap_or_else(|| "Unknown".into())
            ),
            Error::NotSupported(from, to) => {
                write!(f, "Serializing {} as {} is not supported", from, to)
            }
        }
    }
}

impl ser::StdError for Error {}

impl ser::Error for Error {
    fn custom<T>(msg: T) -> Self
    where
        T: fmt::Display,
    {
        Error::Ser(msg.to_string())
    }
}

// adapted from https://github.com/paritytech/parity-scale-codec/blob/master/src/compact.rs#L336
#[allow(clippy::all)]
fn compact_number(n: usize, mut dest: impl BufMut) {
    match n {
        0..=0b0011_1111 => dest.put_u8((n as u8) << 2),
        0..=0b0011_1111_1111_1111 => dest.put_u16_le(((n as u16) << 2) | 0b01),
        0..=0b0011_1111_1111_1111_1111_1111_1111_1111 => dest.put_u32_le(((n as u32) << 2) | 0b10),
        _ => {
            let bytes_needed = 8 - n.leading_zeros() / 8;
            assert!(
                bytes_needed >= 4,
                "Previous match arm matches anyting less than 2^30; qed"
            );
            dest.put_u8(0b11 + ((bytes_needed - 4) << 2) as u8);
            let mut v = n;
            for _ in 0..bytes_needed {
                dest.put_u8(v as u8);
                v >>= 8;
            }
            assert_eq!(
                v, 0,
                "shifted sufficient bits right to lead only leading zeros; qed"
            )
        }
    }
}

// nightly only
fn type_name_of_val<T: ?Sized>(_val: &T) -> &'static str {
    core::any::type_name::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::Encode;
    use core::mem::size_of;
    use scale_info::{meta_type, Registry, TypeInfo};
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
        const INPUT: u64 = 0xFF_EE_DD_CC__BB_AA_99_88;
        let mut out = [0u8; size_of::<u64>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn primitive_u128() -> Result<()> {
        const INPUT: u128 = 0xFF_EE_DD_CC__BB_AA_99_88__77_66_55_44__33_22_11_00;
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

    fn register<T>(_ty: &T) -> (TypeId, PortableRegistry)
    where
        T: TypeInfo + 'static,
    {
        let mut reg = Registry::new();
        let sym = reg.register_type(&meta_type::<T>());
        (sym.id(), reg.into())
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
        struct Foo<'a> {
            a: Vec<String>,
            b: (Bar<'a>, Bar<'a>, Bar<'a>),
        }
        #[derive(Debug, Serialize, Encode, TypeInfo)]
        enum Bar<'a> {
            A { thing: &'a str },
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
}
