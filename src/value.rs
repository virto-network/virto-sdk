use crate::{EnumVariant, SerdeType};
use bytes::{Buf, Bytes};
use core::convert::TryInto;
use core::str;
use scale_info::{prelude::*, Type, TypeDef, TypeDefPrimitive as Primitive};
use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::Serialize;

/// A container for SCALE encoded data that can serialize types
/// directly with the help of a type registry and without using an
/// intermediate representation that requires allocating data.
#[derive(Debug)]
pub struct Value {
    data: Bytes,
    ty: Type,
}

impl Value {
    pub fn new(data: impl Into<Bytes>, ty: Type) -> Self {
        Value {
            data: data.into(),
            ty,
        }
    }

    fn chunk(mut data: impl Buf, ty: Type) -> Self {
        let size = type_len(&ty, data.chunk());
        Value::new(data.copy_to_bytes(size), ty)
    }

    #[inline]
    fn ty_name(&self) -> &'static str {
        self.ty.path().segments().last().copied().unwrap_or("")
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut data = self.data.clone();

        use SerdeType::*;
        match (&self.ty).into() {
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
            Bytes => ser.serialize_bytes(data.chunk()),
            Char => ser.serialize_char(char::from_u32(data.get_u32_le()).unwrap()),
            Str => {
                let (_, s) = sequence_len(data.chunk());
                data.advance(s);
                ser.serialize_str(str::from_utf8(data.chunk()).unwrap())
            }
            Sequence(ty) => {
                let (len, p_size) = sequence_len(data.chunk());
                data.advance(p_size);

                let mut seq = ser.serialize_seq(Some(len))?;
                for _ in 0..len {
                    seq.serialize_element(&Value::chunk(&mut data, ty.clone()))?;
                }
                seq.end()
            }
            Map(ty_k, ty_v) => {
                let (len, p_size) = sequence_len(data.chunk());
                data.advance(p_size);

                let mut state = ser.serialize_map(Some(len))?;
                for _ in 0..len {
                    let key = Value::chunk(&mut data, ty_k.clone());
                    let val = Value::chunk(&mut data, ty_v.clone());
                    state.serialize_entry(&key, &val)?;
                }
                state.end()
            }
            Tuple(t) => {
                let mut state = ser.serialize_tuple(t.len())?;
                for i in 0..t.len() {
                    state.serialize_element(&Value::chunk(&mut data, t.type_info(i)))?;
                }
                state.end()
            }
            Struct(fields) => {
                let mut state = ser.serialize_struct(self.ty_name(), fields.len())?;
                for f in fields {
                    state.serialize_field(
                        f.name().unwrap(),
                        &Value::chunk(&mut data, f.ty().type_info()),
                    )?;
                }
                state.end()
            }
            StructUnit => ser.serialize_unit_struct(self.ty_name()),
            StructNewType(ty) => {
                ser.serialize_newtype_struct(self.ty_name(), &Value::chunk(&mut data, ty))
            }
            StructTuple(fields) => {
                let mut state = ser.serialize_tuple_struct(self.ty_name(), fields.len())?;
                for f in fields {
                    state.serialize_field(&Value::chunk(&mut data, f.ty().type_info()))?;
                }
                state.end()
            }
            ty @ Variant(_, _) => match ty.pick_variant(data.get_u8()) {
                EnumVariant::OptionNone => ser.serialize_none(),
                EnumVariant::OptionSome(ty) => ser.serialize_some(&Value::chunk(&mut data, ty)),
                EnumVariant::Unit(v) => {
                    ser.serialize_unit_variant(self.ty_name(), v.index().into(), v.name())
                }
                EnumVariant::NewType(v) => {
                    let ty = v.fields().first().unwrap().ty().type_info();
                    ser.serialize_newtype_variant(
                        self.ty_name(),
                        v.index().into(),
                        v.name(),
                        &Value::chunk(&mut data, ty),
                    )
                }

                EnumVariant::Tuple(v) => {
                    let mut s = ser.serialize_tuple_variant(
                        self.ty_name(),
                        v.index().into(),
                        v.name(),
                        v.fields().len(),
                    )?;
                    for f in v.fields() {
                        s.serialize_field(&Value::chunk(&mut data, f.ty().type_info()))?;
                    }
                    s.end()
                }
                EnumVariant::Struct(v) => {
                    let mut s = ser.serialize_struct_variant(
                        self.ty_name(),
                        v.index().into(),
                        v.name(),
                        v.fields().len(),
                    )?;
                    for f in v.fields() {
                        let ty = f.ty().type_info();
                        s.serialize_field(f.name().unwrap(), &Value::chunk(&mut data, ty))?;
                    }
                    s.end()
                }
            },
        }
    }
}

fn type_len(ty: &Type, data: &[u8]) -> usize {
    match ty.type_def() {
        TypeDef::Primitive(p) => match p {
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
                let (l, p_size) = sequence_len(data);
                l + p_size
            }
            _ => unimplemented!(),
        },
        TypeDef::Composite(c) => c
            .fields()
            .iter()
            .fold(0, |c, f| c + type_len(&f.ty().type_info(), &data[c..])),
        TypeDef::Variant(e) => {
            let var = e
                .variants()
                .iter()
                .find(|v| v.index() == data[0])
                .expect("variant");

            if var.fields().is_empty() {
                1 // unit variant
            } else {
                var.fields()
                    .iter()
                    .fold(1, |c, f| c + type_len(&f.ty().type_info(), &data[c..]))
            }
        }
        TypeDef::Sequence(s) => {
            let (len, prefix_size) = sequence_len(data);
            let ty = s.type_param().type_info();
            (0..len).fold(prefix_size, |c, _| c + type_len(&ty, &data[c..]))
        }
        TypeDef::Array(a) => a.len().try_into().unwrap(),
        TypeDef::Tuple(t) => t.fields().len(),
        TypeDef::Compact(_) => compact_len(data),
        TypeDef::BitSequence(_) => unimplemented!(),
    }
}

#[inline]
fn compact_len(data: &[u8]) -> usize {
    match data[0] % 0b100 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => todo!(),
    }
}

fn sequence_len(data: &[u8]) -> (usize, usize) {
    // need to peek at the data to know the length of sequence
    // first byte(s) gives us a hint of the(compact encoded) length
    // https://substrate.dev/docs/en/knowledgebase/advanced/codec#compactgeneral-integers
    let len = compact_len(data);
    (
        match len {
            1 => (data[0] >> 2).into(),
            2 => u16::from_le_bytes([(data[0] >> 2), data[1]]).into(),
            4 => u32::from_le_bytes([(data[0] >> 2), data[1], data[2], data[3]])
                .try_into()
                .unwrap(),

            _ => todo!(),
        },
        len,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use anyhow::Error;
    use codec::Encode;
    use scale_info::{
        prelude::{string::String, vec::Vec},
        TypeInfo,
    };
    use serde_json::to_value;

    #[test]
    fn serialize_u8() -> Result<(), Error> {
        let extract_value = u8::MAX;
        let data = extract_value.encode();
        let info = u8::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16() -> Result<(), Error> {
        let extract_value = u16::MAX;
        let data = extract_value.encode();
        let info = u16::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32() -> Result<(), Error> {
        let extract_value = u32::MAX;
        let data = extract_value.encode();
        let info = u32::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u64() -> Result<(), Error> {
        let extract_value = u64::MAX;
        let data = extract_value.encode();
        let info = u64::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i16() -> Result<(), Error> {
        let extract_value = i16::MAX;
        let data = extract_value.encode();
        let info = i16::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i32() -> Result<(), Error> {
        let extract_value = i32::MAX;
        let data = extract_value.encode();
        let info = i32::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i64() -> Result<(), Error> {
        let extract_value = i64::MAX;
        let data = extract_value.encode();
        let info = i64::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_bool() -> Result<(), Error> {
        let extract_value = true;
        let data = extract_value.encode();
        let info = bool::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    // `char` not supported?
    // #[test]
    // fn serialize_char() -> Result<(), Error> {
    //     let extract_value = '⚖';
    //     let data = extract_value.encode();
    //     let info = char::type_info();
    //     let val = Value::new(data, info);
    //     assert_eq!(to_value(val)?, to_value(extract_value)?);
    //     Ok(())
    // }

    #[test]
    fn serialize_u8array() -> Result<(), Error> {
        let extract_value: Vec<u8> = [2u8, u8::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u8>::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16array() -> Result<(), Error> {
        let extract_value: Vec<u16> = [2u16, u16::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u16>::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32array() -> Result<(), Error> {
        let extract_value: Vec<u32> = [2u32, u32::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u32>::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple() -> Result<(), Error> {
        let extract_value: (i64, Vec<String>, bool) = (
            i64::MIN,
            vec!["hello".into(), "big".into(), "world".into()],
            true,
        );
        let data = extract_value.encode();
        let info = <(i64, Vec<String>, bool)>::type_info();
        let val = Value::new(data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u32struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u32,
            baz: u32,
        }
        let extract_value = Foo {
            bar: 123,
            baz: u32::MAX,
        };
        let data = extract_value.encode();
        let info = Foo::type_info();
        let val = Value::new(data, info);

        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u8struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u8,
            baz: u8,
        }
        let extract_value = Foo {
            bar: 123,
            baz: u8::MAX,
        };
        let data = extract_value.encode();
        let info = Foo::type_info();
        let val = Value::new(data, info);

        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u64struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u64,
            baz: u64,
        }
        let extract_value = Foo {
            bar: 123,
            baz: u64::MAX,
        };
        let data = extract_value.encode();
        let info = Foo::type_info();
        let val = Value::new(data, info);

        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_map() -> Result<(), Error> {
        let value = {
            let mut m = BTreeMap::<String, i32>::new();
            m.insert("foo".into(), i32::MAX);
            m.insert("bar".into(), i32::MIN);
            m
        };

        let data = value.encode();
        let info = BTreeMap::<String, i32>::type_info();
        let val = Value::new(data, info);

        assert_eq!(to_value(val)?, to_value(value)?);
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
        let expected = Foo {
            bar: [Bar::That(i16::MAX), Bar::This].into(),
            baz: Some(Baz("aliquam malesuada bibendum arcu vitae".into())),
            lol: b"\0xFFsome stuff\0x00",
        };
        let data = expected.encode();
        let info = Foo::type_info();
        let out = Value::new(data, info);

        assert_eq!(to_value(out)?, to_value(expected)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple_struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo<'a>([u8; 4], (bool, Option<()>), Baz<'a>, Baz<'a>);

        #[derive(Encode, Serialize, TypeInfo)]
        struct Bar;

        #[derive(Encode, Serialize, TypeInfo)]
        enum Baz<'a> {
            A(Bar),
            B { bb: &'a str },
        }

        let extract_value = Foo(
            [1, 2, 3, 4],
            (false, None),
            Baz::A(Bar),
            Baz::B { bb: "lol" },
        );
        let data = extract_value.encode();
        let info = Foo::type_info();
        let val = Value::new(data, info);

        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }
}
