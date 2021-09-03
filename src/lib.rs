use byteorder::{ByteOrder, LE};
use scale_info::{prelude::*, Field, Type, TypeDef, TypeDefPrimitive as Primitive};
use serde::ser::{SerializeSeq, SerializeStruct, SerializeTuple, SerializeTupleStruct};
use serde::Serialize;
use std::convert::TryInto;
use std::str;

/// A non-owning container for SCALE encoded data that can serialize types
/// directly with the help of a type registry and without using an intermediate
/// representation that requires allocating data.
pub struct Value<'a> {
    data: &'a [u8],
    ty: Type,
}

impl<'a> Value<'a> {
    pub fn new(data: &'a [u8], ty: Type) -> Self {
        Value { data, ty }
    }
}

impl<'a> Serialize for Value<'a> {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut data = self.data;
        let name = ty_name(&self.ty);

        match self.ty.type_def() {
            TypeDef::Composite(cmp) => {
                let fields = cmp.fields();
                // Differentiating between a tuple struct and a normal one
                if fields.first().and_then(Field::name).is_none() {
                    let state = ser.serialize_tuple_struct(name, fields.len())?;
                    // TODO!;
                    state.end()
                } else {
                    let mut state = ser.serialize_struct(&name, fields.len())?;
                    for f in fields {
                        let ty = f.ty().type_info();
                        let size = type_byte_count(&ty, data);
                        let (val, reminder) = data.split_at(size);
                        data = reminder;
                        state.serialize_field(f.name().unwrap(), &Value::new(val, ty))?;
                    }
                    state.end()
                }
            }
            TypeDef::Variant(enu) => {
                for v in enu.variants() {
                    if v.fields().is_empty() {
                        ser.serialize_unit_variant(name, v.index().into(), v.name())?;
                        todo!()
                    } else {
                        for _f in v.fields() {
                            todo!()
                        }
                    }
                }
                todo!()
            }
            TypeDef::Sequence(seq) => {
                let ty = seq.type_param().type_info();
                let (len, p_size) = sequence_len(data);
                let mut data = &data[p_size..];
                let mut seq = ser.serialize_seq(Some(len))?;

                for _ in 0..len {
                    let ts = type_byte_count(&ty, data);
                    seq.serialize_element(&Value::new(&data[..ts], ty.clone()))?;
                    data = &data[ts..];
                }
                seq.end()
            }
            TypeDef::Array(_) => todo!(),
            TypeDef::Tuple(tup) => {
                let mut state = ser.serialize_tuple(tup.fields().len())?;
                for f in tup.fields() {
                    let ty = f.type_info();
                    let (val, reminder) = data.split_at(type_byte_count(&ty, data));
                    data = reminder;
                    state.serialize_element(&Value::new(val, ty))?;
                }
                state.end()
            }
            TypeDef::Primitive(prm) => {
                match prm {
                    Primitive::U8 => ser.serialize_u8(data[0]),
                    Primitive::U16 => ser.serialize_u16(LE::read_u16(data)),
                    Primitive::U32 => ser.serialize_u32(LE::read_u32(data)),
                    Primitive::U64 => ser.serialize_u64(LE::read_u64(data)),
                    Primitive::U128 => ser.serialize_u128(LE::read_u128(data)),
                    Primitive::I8 => ser.serialize_i8(i8::from_le_bytes([data[0]])),
                    Primitive::I16 => ser.serialize_i16(LE::read_i16(data)),
                    Primitive::I32 => ser.serialize_i32(LE::read_i32(data)),
                    Primitive::I64 => ser.serialize_i64(LE::read_i64(data)),
                    Primitive::I128 => ser.serialize_i128(LE::read_i128(data)),
                    Primitive::Bool => ser.serialize_bool(data[0] != 0),
                    Primitive::Str => {
                        let (_, s) = sequence_len(data);
                        ser.serialize_str(str::from_utf8(&data[s..]).unwrap())
                    }
                    // TODO: test if statements from here on actually work
                    Primitive::Char => {
                        let n = LE::read_u32(data);
                        ser.serialize_char(char::from_u32(n).unwrap())
                    }
                    _ => ser.serialize_bytes(data),
                }
            }
            TypeDef::Compact(_) => todo!(),
            TypeDef::BitSequence(_) => todo!(),
        }
    }
}

fn ty_name(ty: &Type) -> &'static str {
    ty.path().segments().last().copied().unwrap_or("")
}

fn type_byte_count<'a>(t: &Type, data: &'a [u8]) -> usize {
    match t.type_def() {
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
        TypeDef::Composite(_) => todo!(),
        TypeDef::Variant(_) => todo!(),
        TypeDef::Sequence(s) => {
            let (len, prefix_size) = sequence_len(data);
            let ty = s.type_param().type_info();
            (0..len).fold(prefix_size, |c, _| c + type_byte_count(&ty, &data[c..]))
        }
        TypeDef::Array(_) => todo!(),
        TypeDef::Tuple(_) => todo!(),
        TypeDef::Compact(_) => todo!(),
        TypeDef::BitSequence(_) => todo!(),
    }
}

fn sequence_len(data: &[u8]) -> (usize, usize) {
    // need to peek at the data to know the length of sequence
    // first byte(s) gives us a hint of the(compact encoded) length
    // https://substrate.dev/docs/en/knowledgebase/advanced/codec#compactgeneral-integers
    match data[0] % 0b100 {
        0 => ((data[0] >> 2).into(), 1),
        1 => (u16::from_le_bytes([(data[0] >> 2), data[1]]).into(), 2),
        2 => (
            u32::from_le_bytes([(data[0] >> 2), data[1], data[2], data[3]])
                .try_into()
                .unwrap(),
            4,
        ),
        _ => todo!(),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;
    use parity_scale_codec::Encode;
    use scale_info::TypeInfo;
    use serde_json::to_value;

    #[test]
    fn serialize_u8() -> Result<(), Box<dyn Error>> {
        let extract_value = u8::MAX;
        let data = extract_value.encode();
        let info = u8::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16() -> Result<(), Box<dyn Error>> {
        let extract_value = u16::MAX;
        let data = extract_value.encode();
        let info = u16::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32() -> Result<(), Box<dyn Error>> {
        let extract_value = u32::MAX;
        let data = extract_value.encode();
        let info = u32::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u64() -> Result<(), Box<dyn Error>> {
        let extract_value = u64::MAX;
        let data = extract_value.encode();
        let info = u64::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i16() -> Result<(), Box<dyn Error>> {
        let extract_value = i16::MAX;
        let data = extract_value.encode();
        let info = i16::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i32() -> Result<(), Box<dyn Error>> {
        let extract_value = i32::MAX;
        let data = extract_value.encode();
        let info = i32::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i64() -> Result<(), Box<dyn Error>> {
        let extract_value = i64::MAX;
        let data = extract_value.encode();
        let info = i64::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_bool() -> Result<(), Box<dyn Error>> {
        let extract_value = true;
        let data = extract_value.encode();
        let info = bool::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u8array() -> Result<(), Box<dyn Error>> {
        let extract_value: Vec<u8> = [2u8, u8::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u8>::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16array() -> Result<(), Box<dyn Error>> {
        let extract_value: Vec<u16> = [2u16, u16::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u16>::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32array() -> Result<(), Box<dyn Error>> {
        let extract_value: Vec<u32> = [2u32, u32::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u32>::type_info();
        let val = Value::new(&data, info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple() -> Result<(), Box<dyn Error>> {
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
    fn serialize_simple_u32struct() -> Result<(), Box<dyn Error>> {
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
    fn serialize_simple_u8struct() -> Result<(), Box<dyn Error>> {
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
    fn serialize_simple_u64struct() -> Result<(), Box<dyn Error>> {
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
    fn serialize_complex_struct() -> Result<(), Box<dyn Error>> {
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
}
