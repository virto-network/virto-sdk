use crate::{EnumVariant, SerdeType};
use byteorder::{ByteOrder, LE};
use core::cell::Cell;
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
pub struct Value<'a> {
    data: &'a [u8],
    ty: Type,
    idx: Cell<usize>,
}

impl<'a> Value<'a> {
    pub fn new(data: &'a [u8], ty: Type) -> Self {
        Value {
            data,
            ty,
            idx: 0.into(),
        }
    }
}

impl<'a> Serialize for Value<'a> {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let name = self.ty_name();
        let data = self.data_left();
        let ty = SerdeType::from(&self.ty);

        use SerdeType::*;
        match ty {
            Bool => ser.serialize_bool(data[0] != 0),
            U8 => ser.serialize_u8(data[0]),
            U16 => ser.serialize_u16(LE::read_u16(data)),
            U32 => ser.serialize_u32(LE::read_u32(data)),
            U64 => ser.serialize_u64(LE::read_u64(data)),
            U128 => ser.serialize_u128(LE::read_u128(data)),
            I8 => ser.serialize_i8(i8::from_le_bytes([data[0]])),
            I16 => ser.serialize_i16(LE::read_i16(data)),
            I32 => ser.serialize_i32(LE::read_i32(data)),
            I64 => ser.serialize_i64(LE::read_i64(data)),
            I128 => ser.serialize_i128(LE::read_i128(data)),
            Bytes => unimplemented!(),
            Char => {
                let n = LE::read_u32(data);
                ser.serialize_char(char::from_u32(n).unwrap())
            }
            Str => {
                let (_, s) = sequence_len(data);
                self.advance_idx(s);
                ser.serialize_str(str::from_utf8(self.data_left()).unwrap())
            }
            Sequence(ty) => {
                let (len, p_size) = sequence_len(self.data_left());
                self.advance_idx(p_size);

                let mut seq = ser.serialize_seq(Some(len))?;
                for _ in 0..len {
                    seq.serialize_element(&self.sub_value(ty.clone()))?;
                }
                seq.end()
            }
            Map(ty_k, ty_v) => {
                let (len, p_size) = sequence_len(self.data_left());
                self.advance_idx(p_size);

                let mut state = ser.serialize_map(Some(len))?;
                for _ in 0..len {
                    let key = &self.sub_value(ty_k.clone());
                    let val = &self.sub_value(ty_v.clone());
                    state.serialize_entry(key, val)?;
                }
                state.end()
            }
            Tuple(t) => {
                let mut state = ser.serialize_tuple(t.len())?;
                for i in 0..t.len() {
                    state.serialize_element(&self.sub_value(t.type_info(i)))?;
                }
                state.end()
            }
            Struct(fields) => {
                let mut state = ser.serialize_struct(name, fields.len())?;
                for f in fields {
                    state
                        .serialize_field(f.name().unwrap(), &self.sub_value(f.ty().type_info()))?;
                }
                state.end()
            }
            StructUnit => ser.serialize_unit_struct(name),
            StructNewType => unimplemented!(),
            StructTuple(fields) => {
                let mut state = ser.serialize_tuple_struct(name, fields.len())?;
                for f in fields {
                    state.serialize_field(&self.sub_value(f.ty().type_info()))?;
                }
                state.end()
            }
            Variant(_, _) => match ty.pick_variant(data[0]) {
                EnumVariant::OptionNone => ser.serialize_none(),
                EnumVariant::OptionSome(ty) => {
                    self.advance_idx(1);
                    ser.serialize_some(&self.sub_value(ty))
                }
                EnumVariant::Unit(v) => {
                    ser.serialize_unit_variant(name, v.index().into(), v.name())
                }
                EnumVariant::NewType(v) => {
                    self.advance_idx(1);
                    let ty = v.fields().first().unwrap().ty().type_info();
                    ser.serialize_newtype_variant(
                        name,
                        v.index().into(),
                        v.name(),
                        &self.sub_value(ty),
                    )
                }

                EnumVariant::Tuple(v) => {
                    self.advance_idx(1);
                    let mut s = ser.serialize_tuple_variant(
                        name,
                        v.index().into(),
                        v.name(),
                        v.fields().len(),
                    )?;
                    for f in v.fields() {
                        s.serialize_field(&self.sub_value(f.ty().type_info()))?;
                    }
                    s.end()
                }
                EnumVariant::Struct(v) => {
                    self.advance_idx(1);
                    let mut s = ser.serialize_struct_variant(
                        name,
                        v.index().into(),
                        v.name(),
                        v.fields().len(),
                    )?;
                    for f in v.fields() {
                        s.serialize_field(f.name().unwrap(), &self.sub_value(f.ty().type_info()))?;
                    }
                    s.end()
                }
            },
        }
    }
}

impl<'a> Value<'a> {
    fn data_left(&self) -> &[u8] {
        &self.data[self.idx.get()..]
    }

    fn sub_value(&self, ty: Type) -> Value {
        let size = ty_data_size(&ty, self.data_left());
        Value::new(self.extract_data(size), ty)
    }

    fn extract_data(&self, end: usize) -> &[u8] {
        let (data, _) = &self.data_left().split_at(end);
        self.advance_idx(end);
        data
    }

    fn advance_idx(&self, n: usize) {
        self.idx.set(self.idx.get() + n)
    }

    fn ty_name(&self) -> &'static str {
        self.ty.path().segments().last().copied().unwrap_or("")
    }
}

fn ty_data_size(ty: &Type, data: &[u8]) -> usize {
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
            .fold(0, |c, f| c + ty_data_size(&f.ty().type_info(), &data[c..])),
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
                    .fold(1, |c, f| c + ty_data_size(&f.ty().type_info(), &data[c..]))
            }
        }
        TypeDef::Sequence(s) => {
            let (len, prefix_size) = sequence_len(data);
            let ty = s.type_param().type_info();
            (0..len).fold(prefix_size, |c, _| c + ty_data_size(&ty, &data[c..]))
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
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16() -> Result<(), Error> {
        let extract_value = u16::MAX;
        let data = extract_value.encode();
        let info = u16::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32() -> Result<(), Error> {
        let extract_value = u32::MAX;
        let data = extract_value.encode();
        let info = u32::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u64() -> Result<(), Error> {
        let extract_value = u64::MAX;
        let data = extract_value.encode();
        let info = u64::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i16() -> Result<(), Error> {
        let extract_value = i16::MAX;
        let data = extract_value.encode();
        let info = i16::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i32() -> Result<(), Error> {
        let extract_value = i32::MAX;
        let data = extract_value.encode();
        let info = i32::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i64() -> Result<(), Error> {
        let extract_value = i64::MAX;
        let data = extract_value.encode();
        let info = i64::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_bool() -> Result<(), Error> {
        let extract_value = true;
        let data = extract_value.encode();
        let info = bool::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    // `char` not supported?
    // #[test]
    // fn serialize_char() -> Result<(), Error> {
    //     let extract_value = '⚖';
    //     let data = extract_value.encode();
    //     let info = char::type_info();
    //     let val = Value::new(&data, info);
    //     assert_eq!(to_value(val)?, to_value(extract_value)?);
    //     Ok(())
    // }

    #[test]
    fn serialize_u8array() -> Result<(), Error> {
        let extract_value: Vec<u8> = [2u8, u8::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u8>::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16array() -> Result<(), Error> {
        let extract_value: Vec<u16> = [2u16, u16::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u16>::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32array() -> Result<(), Error> {
        let extract_value: Vec<u32> = [2u32, u32::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u32>::type_info();
        let val = Value::new(&data, info);
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
        let val = Value::new(&data, info);
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
        let val = Value::new(&data, info);

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
        let val = Value::new(&data, info);

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
        let val = Value::new(&data, info);

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
        let val = Value::new(&data, info);

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
        struct Foo {
            bar: Vec<Bar>,
            baz: Option<String>,
        }
        let extract_value = Foo {
            bar: [Bar::That(i16::MAX), Bar::This].into(),
            baz: Some("aliquam malesuada bibendum arcu vitae".into()),
        };
        let data = extract_value.encode();
        let info = Foo::type_info();
        let val = Value::new(&data, info);

        assert_eq!(to_value(val)?, to_value(extract_value)?);
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
        let val = Value::new(&data, info);

        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }
}
