use bytes::BufMut;
use core::fmt;
use scale_info::{Type, TypeInfo};
use serde::{ser, Serialize};

use crate::{SerdeType, TupleOrArray};

type Result<T> = core::result::Result<T, Error>;

#[derive(TypeInfo)]
struct Noop;

pub fn to_bytes<B, T>(bytes: B, value: &T) -> Result<()>
where
    T: Serialize,
    B: BufMut,
{
    to_bytes_with_info(bytes, value, None)
}

pub fn to_bytes_with_info<B, T>(writer: B, value: &T, info: impl Into<Option<Type>>) -> Result<()>
where
    T: Serialize,
    B: BufMut,
{
    let mut serializer = Serializer::new(writer, info);
    value.serialize(&mut serializer)?;
    Ok(())
}

pub struct Serializer<B> {
    out: B,
    ty: Option<SerdeType>,
}

impl<B> Serializer<B>
where
    B: BufMut,
{
    pub fn new(out: B, ty: impl Into<Option<Type>>) -> Self {
        Serializer {
            out,
            ty: ty.into().map(SerdeType::from),
        }
    }
}

impl<'a, B> ser::Serializer for &'a mut Serializer<B>
where
    B: BufMut,
{
    type Ok = ();
    type Error = Error;

    type SerializeSeq = TypedSerializer<'a, B>;
    type SerializeTuple = TypedSerializer<'a, B>;
    type SerializeTupleStruct = TypedSerializer<'a, B>;
    type SerializeTupleVariant = TypedSerializer<'a, B>;
    type SerializeMap = TypedSerializer<'a, B>;
    type SerializeStruct = TypedSerializer<'a, B>;
    type SerializeStructVariant = TypedSerializer<'a, B>;

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
            Some(SerdeType::I8) => self.serialize_i8(v as i8)?,
            Some(SerdeType::I16) => self.serialize_i16(v as i16)?,
            Some(SerdeType::I32) => self.serialize_i32(v as i32)?,
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
        match self.ty {
            Some(SerdeType::U8) => self.serialize_u8(v as u8)?,
            Some(SerdeType::U16) => self.serialize_u16(v as u16)?,
            Some(SerdeType::U32) => self.serialize_u32(v as u32)?,
            _ => {
                self.maybe_some()?;
                self.out.put_u64_le(v);
            }
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
        compact_number(v.len(), &mut self.out);
        self.out.put(v.as_bytes());
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok> {
        self.maybe_some()?;
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
        println!("===== u {:?}\n", self.ty);
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok> {
        self.maybe_some()?;
        println!("===== us {:?}\n", self.ty);
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        __name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok> {
        self.maybe_some()?;
        println!("===== uv {:?}\n", self.ty);
        (variant_index as u8).serialize(self)
    }

    fn serialize_newtype_struct<T: ?Sized>(self, _name: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: Serialize,
    {
        self.maybe_some()?;
        println!("===== ns {:?}\n", self.ty);
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
        println!("===== nv {:?}\n", self.ty);
        self.out.put_u8(variant_index as u8);
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        self.maybe_some()?;
        println!("===== seq {:?}\n", self.ty);
        if !matches!(self.ty, Some(SerdeType::StructTuple(_))) {
            compact_number(len.expect("known length"), &mut self.out);
        }
        Ok(self.into())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        self.maybe_some()?;
        println!("===== tup {:?}\n", self.ty);
        Ok(self.into())
    }

    fn serialize_tuple_struct(
        self,
        __name: &'static str,
        __len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.maybe_some()?;
        println!("===== tups {:?}", self.ty);
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
        println!("===== tupv {:?}", self.ty);
        Ok(self.into())
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        self.maybe_some()?;
        println!("===== m {:?}\n", self.ty);
        if matches!(self.ty, Some(SerdeType::Variant(_, _, _))) {
            // TODO!
            println!("map as variant: {:?}\n", len);
        }
        if !matches!(self.ty, Some(SerdeType::Struct(_))) {
            compact_number(len.expect("known length"), &mut self.out);
        }
        Ok(self.into())
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        self.maybe_some()?;
        println!("===== s {:?}", self.ty);
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
        println!("===== sv {:?}", self.ty);
        Ok(self.into())
    }
}

impl<'a, B> Serializer<B>
where
    B: BufMut,
{
    // A check to run for every serialize fn since any type could be an Option::Some
    // if the type info says its an Option assume its Some and extract the inner type
    fn maybe_some(&mut self) -> Result<()> {
        match &self.ty {
            Some(SerdeType::Variant(ref name, v, _)) if name == "Option" => {
                self.ty = v[1].fields().first().map(|f| f.ty().type_info().into());
                self.out.put_u8(0x01);
            }
            _ => (),
        }
        Ok(())
    }
}

impl<'a, B: 'a> From<&'a mut Serializer<B>> for TypedSerializer<'a, B> {
    fn from(ser: &'a mut Serializer<B>) -> Self {
        use SerdeType::*;

        let t = ser.ty.take();
        match t {
            Some(Struct(fields) | StructTuple(fields)) => {
                Self::Collection(ser, fields.iter().map(|f| f.ty().type_info()).collect())
            }
            Some(Tuple(TupleOrArray::Array(ty, n))) => {
                Self::Collection(ser, (0..n).map(|_| ty.clone()).collect())
            }
            Some(Tuple(TupleOrArray::Tuple(fields))) => Self::Collection(ser, fields.into()),
            Some(Variant(_, variants, _)) => {
                // assuming the variant name is used to select the right variant
                let variants = variants.iter().map(|v| *v.name()).collect();
                Self::Variant {
                    ser,
                    variants,
                    selected: None,
                }
            }
            _ => Self::Empty(ser),
        }
    }
}

pub enum TypedSerializer<'a, B> {
    Empty(&'a mut Serializer<B>),
    Collection(&'a mut Serializer<B>, Vec<Type>),
    Variant {
        ser: &'a mut Serializer<B>,
        variants: Vec<&'a str>,
        selected: Option<u8>,
    },
}

impl<'a, B> TypedSerializer<'a, B> {
    fn serializer(&mut self) -> &mut Serializer<B> {
        match self {
            Self::Empty(ser) | Self::Collection(ser, _) | Self::Variant { ser, .. } => ser,
        }
    }
}

impl<'a, B> ser::SerializeMap for TypedSerializer<'a, B>
where
    B: BufMut,
{
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: ?Sized>(&mut self, key: &T) -> Result<()>
    where
        T: Serialize,
    {
        match self {
            TypedSerializer::Collection(_, _)
            | TypedSerializer::Variant {
                selected: Some(_), ..
            } => Ok(()),
            TypedSerializer::Variant {
                ser,
                variants,
                selected: selected @ None,
            } => {
                let key_data = {
                    let mut s = Serializer::new(vec![], None);
                    key.serialize(&mut s)?;
                    s.out
                };
                let idx = variants
                    .iter()
                    .position(|name| {
                        let mut s = Serializer::new(vec![], None);
                        name.serialize(&mut s).expect("key serialized");
                        s.out == key_data
                    })
                    .expect("key exists") as u8;
                idx.serialize(&mut **ser)?;
                *selected = Some(idx);
                Ok(())
            }
            TypedSerializer::Empty(ser) => key.serialize(&mut **ser),
        }
    }

    fn serialize_value<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        if let TypedSerializer::Collection(ser, types) = self {
            let mut ty = types.remove(0).into();
            // serde_json unwraps newtypes
            if let SerdeType::StructNewType(t) = ty {
                ty = t.into()
            }
            ser.ty = Some(ty);
        };
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, B> ser::SerializeSeq for TypedSerializer<'a, B>
where
    B: BufMut,
{
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        if let Self::Collection(ser, types) = self {
            let mut ty = types.remove(0).into();
            if let SerdeType::StructNewType(t) = ty {
                ty = t.into()
            }
            ser.ty = Some(ty);
        }
        value.serialize(self.serializer())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, B> ser::SerializeStruct for TypedSerializer<'a, B>
where
    B: BufMut,
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

impl<'a, B> ser::SerializeStructVariant for TypedSerializer<'a, B>
where
    B: BufMut,
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

impl<'a, B> ser::SerializeTuple for TypedSerializer<'a, B>
where
    B: BufMut,
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

impl<'a, B> ser::SerializeTupleStruct for TypedSerializer<'a, B>
where
    B: BufMut,
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

impl<'a, B> ser::SerializeTupleVariant for TypedSerializer<'a, B>
where
    B: BufMut,
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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Ser(msg) => write!(f, "{}", msg),
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

#[cfg(test)]
mod tests {
    use super::*;
    use codec::Encode;
    use core::mem::size_of;
    use scale_info::TypeInfo;
    use serde_json::to_value;
    use std::collections::BTreeMap;

    #[test]
    fn test_primitive_u8() -> Result<()> {
        let mut out = [0u8];
        to_bytes(&mut out[..], &123u8)?;

        let expected = [123];

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_primitive_u16() -> Result<()> {
        const INPUT: u16 = 0xFF_EE;
        let mut out = [0u8; size_of::<u16>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_ref(), expected);
        Ok(())
    }

    #[test]
    fn test_primitive_u32() -> Result<()> {
        const INPUT: u32 = 0xFF_EE_DD_CC;
        let mut out = [0u8; size_of::<u32>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_ref(), expected);
        Ok(())
    }

    #[test]
    fn test_primitive_u64() -> Result<()> {
        const INPUT: u64 = 0xFF_EE_DD_CC__BB_AA_99_88;
        let mut out = [0u8; size_of::<u64>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn test_primitive_i16() -> Result<()> {
        const INPUT: i16 = i16::MIN;
        let mut out = [0u8; size_of::<i16>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn test_primitive_i32() -> Result<()> {
        const INPUT: i32 = i32::MIN;
        let mut out = [0u8; size_of::<i32>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn test_primitive_i64() -> Result<()> {
        const INPUT: i64 = i64::MIN;
        let mut out = [0u8; size_of::<i64>()];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn test_primitive_bool() -> Result<()> {
        const INPUT: bool = true;
        let mut out = [0u8];
        let expected = INPUT.encode();

        to_bytes(out.as_mut(), &INPUT)?;

        assert_eq!(out.as_mut(), expected);
        Ok(())
    }

    #[test]
    fn test_str() -> Result<()> {
        const INPUT: &str = "ac orci phasellus egestas tellus rutrum tellus pellentesque";
        let mut out = Vec::<u8>::new();
        let expected = INPUT.encode();

        to_bytes(&mut out, &INPUT)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_bytes() -> Result<()> {
        const INPUT: &[u8] = b"dictumst quisque sagittis purus sit amet volutpat consequat";
        let mut out = Vec::<u8>::new();
        let expected = INPUT.encode();

        to_bytes(&mut out, &INPUT)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_tuple_simple() -> Result<()> {
        const INPUT: (u8, bool, u64) = (0xD0, false, u64::MAX);
        let mut out = Vec::<u8>::new();
        let expected = INPUT.encode();

        to_bytes(&mut out, &INPUT)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_enum_simple() -> Result<()> {
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
    fn test_tuple_enum_mix() -> Result<()> {
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
    fn test_struct_simple() -> Result<()> {
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
    fn test_vec_simple() -> Result<()> {
        let input: Vec<String> = vec!["hello".into(), "beautiful".into(), "people".into()];
        let mut out = Vec::<u8>::new();
        let expected = input.encode();

        to_bytes(&mut out, &input)?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_struct_mix() -> Result<()> {
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

    #[test]
    fn test_json_simple() -> Result<()> {
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

        let json_input = to_value(&input).unwrap();
        println!("{:?}\n", input);
        println!("{:?}\n", json_input);
        to_bytes_with_info(&mut out, &json_input, Foo::type_info())?;

        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_json_mix() -> Result<()> {
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
                        h.insert("key".into(), false);
                        h
                    },
                    i64::MIN,
                ),
            ),
        };
        let mut out = Vec::<u8>::new();
        let expected = input.encode();

        let json_input = to_value(&input).unwrap();
        println!("{:?}\n", input);
        println!("{:?}\n", json_input);
        to_bytes_with_info(&mut out, &json_input, Foo::type_info())?;

        assert_eq!(out, expected);
        Ok(())
    }
}
