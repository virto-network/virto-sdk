use byteorder::{ByteOrder, LE};
use scale_info::{prelude::*, Field, Type, TypeDef, TypeDefPrimitive as Primitive};
use serde::ser::{
    SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple, SerializeTupleStruct,
    SerializeTupleVariant,
};
use serde::Serialize;
use std::convert::TryInto;
use std::str;

/// A container for SCALE encoded data that can serialize types
/// directly with the help of a type registry and without using an
/// intermediate representation that requires allocating data.
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
                if fields.len() == 0 {
                    return ser.serialize_unit_struct(name);
                }
                if is_tuple(fields) {
                    let mut state = ser.serialize_tuple_struct(name, fields.len())?;
                    for f in fields {
                        let ty = f.ty().type_info();
                        let size = ty_data_size(&ty, data);
                        let (val, reminder) = data.split_at(size);
                        data = reminder;
                        state.serialize_field(&Value::new(val, ty))?;
                    }
                    state.end()
                } else {
                    let mut state = ser.serialize_struct(&name, fields.len())?;
                    for f in fields {
                        let ty = f.ty().type_info();
                        let size = ty_data_size(&ty, data);
                        let (val, reminder) = data.split_at(size);
                        data = reminder;
                        state.serialize_field(f.name().unwrap(), &Value::new(val, ty))?;
                    }
                    state.end()
                }
            }
            TypeDef::Variant(enu) => {
                let (idx, d) = data.split_at(1);
                data = d;
                let idx = idx[0];
                let var = enu
                    .variants()
                    .iter()
                    .find(|v| v.index() == idx)
                    .expect("variant");

                let fields = var.fields();
                if fields.is_empty() {
                    if name == "Option" && *var.name() == "None" {
                        return ser.serialize_none();
                    }
                    ser.serialize_unit_variant(name, var.index().into(), var.name())
                } else if is_tuple(fields) {
                    if fields.len() == 1 {
                        let ty = fields.first().unwrap().ty().type_info();
                        let size = ty_data_size(&ty, data);
                        let val = Value::new(&data[..size], ty);
                        if name == "Option" && *var.name() == "Some" {
                            return ser.serialize_some(&val);
                        }
                        return ser.serialize_newtype_variant(name, idx.into(), var.name(), &val);
                    }
                    let mut s =
                        ser.serialize_tuple_variant(name, idx.into(), var.name(), fields.len())?;
                    for f in var.fields() {
                        let ty = f.ty().type_info();
                        let size = ty_data_size(&ty, data);
                        let (val, reminder) = data.split_at(size);
                        data = reminder;
                        s.serialize_field(&Value::new(val, ty))?;
                    }
                    s.end()
                } else {
                    let mut s = ser.serialize_struct_variant(
                        name,
                        var.index().into(),
                        var.name(),
                        fields.len(),
                    )?;
                    for f in var.fields() {
                        let ty = f.ty().type_info();
                        let size = ty_data_size(&ty, data);
                        let (val, reminder) = data.split_at(size);
                        data = reminder;
                        s.serialize_field(f.name().unwrap(), &Value::new(val, ty))?;
                    }
                    s.end()
                }
            }
            TypeDef::Sequence(seq) => {
                let ty = seq.type_param().type_info();
                let (len, p_size) = sequence_len(data);
                let mut data = &data[p_size..];
                let mut seq = ser.serialize_seq(Some(len))?;

                for _ in 0..len {
                    let ts = ty_data_size(&ty, data);
                    seq.serialize_element(&Value::new(&data[..ts], ty.clone()))?;
                    data = &data[ts..];
                }
                seq.end()
            }
            TypeDef::Array(arr) => {
                let mut s = ser.serialize_tuple(arr.len().try_into().unwrap())?;
                let ty = arr.type_param().type_info();
                for _ in 0..arr.len() {
                    let (val, reminder) = data.split_at(ty_data_size(&ty, data));
                    data = reminder;
                    s.serialize_element(&Value::new(val, ty.clone()))?;
                }
                s.end()
            }
            TypeDef::Tuple(tup) => {
                let mut state = ser.serialize_tuple(tup.fields().len())?;
                for f in tup.fields() {
                    let ty = f.type_info();
                    let (val, reminder) = data.split_at(ty_data_size(&ty, data));
                    data = reminder;
                    state.serialize_element(&Value::new(val, ty))?;
                }
                state.end()
            }
            TypeDef::Primitive(prm) => match prm {
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
                Primitive::Char => {
                    let n = LE::read_u32(data);
                    ser.serialize_char(char::from_u32(n).unwrap())
                }
                Primitive::U256 => unimplemented!(),
                Primitive::I256 => unimplemented!(),
            },
            TypeDef::Compact(_) => todo!(),
            TypeDef::BitSequence(_) => todo!(),
        }
    }
}

fn is_tuple(fields: &[Field]) -> bool {
    fields.first().and_then(Field::name).is_none()
}

fn ty_name(ty: &Type) -> &'static str {
    ty.path().segments().last().copied().unwrap_or("")
}

fn ty_data_size<'a>(ty: &Type, data: &'a [u8]) -> usize {
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
        TypeDef::BitSequence(_) => todo!(),
    }
}

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

    // `char` not supported?
    // #[test]
    // fn serialize_char() -> Result<(), Box<dyn Error>> {
    //     let extract_value = '⚖';
    //     let data = extract_value.encode();
    //     let info = char::type_info();
    //     let val = Value::new(&data, info);
    //     assert_eq!(to_value(val)?, to_value(extract_value)?);
    //     Ok(())
    // }

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
    fn serialize_complex_struct_with_enum() -> Result<(), Box<dyn Error>> {
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
    fn serialize_tuple_struct() -> Result<(), Box<dyn Error>> {
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
