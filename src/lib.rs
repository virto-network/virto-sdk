use byteorder::{ByteOrder, LE};
use scale_info::{Field, TypeDefPrimitive as Primitive};
use scale_info::{Type, TypeDef};
use serde::ser::{SerializeStruct, SerializeTupleStruct};
use serde::Serialize;
use std::str;

/// A non-owning container for SCALE encoded data that can serialize types
/// directly with the help of a type registry and without using an intermediate
/// representation that requires allocating data.
pub struct Value<'a> {
    data: &'a [u8],
    info: &'a Type,
}

impl<'a> Value<'a> {
    pub fn new(data: &'a [u8], info: &'a Type) -> Self {
        Value { data, info }
    }
}

impl<'a> Serialize for Value<'a> {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let data = self.data;
        let ty = self.info;
        let name = ty_name(ty);

        match ty.type_def() {
            TypeDef::Composite(def) => {
                let fields = def.fields();
                // Differentiating between a tuple struct and a normal one
                if fields.first().and_then(Field::name).is_none() {
                    let state = ser.serialize_tuple_struct(name, fields.len())?;
                    // TODO!;
                    state.end()
                } else {
                    let mut state = ser.serialize_struct(&name, fields.len())?;
                    for f in fields {
                        let v = extract_value(data, f.ty().type_info());
                        state.serialize_field(f.name().unwrap(), &v)?;
                    }
                    state.end()
                }
            }
            TypeDef::Variant(en) => {
                for v in en.variants() {
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
            TypeDef::Sequence(_) => todo!(),
            TypeDef::Array(_) => todo!(),
            TypeDef::Tuple(_) => todo!(),
            TypeDef::Primitive(def) => {
                match def {
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

                    // TODO: test if statements from here on actually work
                    Primitive::Char => {
                        let n = LE::read_u32(data);
                        ser.serialize_char(char::from_u32(n).unwrap())
                    }
                    Primitive::Str => ser.serialize_str(str::from_utf8(data).unwrap()),
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

// (TODO) Construct a Value slicing
fn extract_value(_data: &[u8], ty: Type) -> Value {
    use TypeDef::*;
    match ty.type_def() {
        Composite(_def) => todo!(),
        Variant(_def) => todo!(),
        Sequence(_def) => todo!(),
        Array(_def) => todo!(),
        Tuple(_def) => todo!(),
        Primitive(_def) => todo!(),
        Compact(_def) => todo!(),
        BitSequence(_def) => todo!(),
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
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16() -> Result<(), Box<dyn Error>> {
        let extract_value = u16::MAX;
        let data = extract_value.encode();
        let info = u16::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32() -> Result<(), Box<dyn Error>> {
        let extract_value = u32::MAX;
        let data = extract_value.encode();
        let info = u32::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u64() -> Result<(), Box<dyn Error>> {
        let extract_value = u64::MAX;
        let data = extract_value.encode();
        let info = u64::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i16() -> Result<(), Box<dyn Error>> {
        let extract_value = i16::MAX;
        let data = extract_value.encode();
        let info = i16::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i32() -> Result<(), Box<dyn Error>> {
        let extract_value = i32::MAX;
        let data = extract_value.encode();
        let info = i32::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i64() -> Result<(), Box<dyn Error>> {
        let extract_value = i64::MAX;
        let data = extract_value.encode();
        let info = i64::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_bool() -> Result<(), Box<dyn Error>> {
        let extract_value = true;
        let data = extract_value.encode();
        let info = bool::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u8array() -> Result<(), Box<dyn Error>> {
        let extract_value: Vec<u8> = [2u8, u8::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u8>::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16array() -> Result<(), Box<dyn Error>> {
        let extract_value: Vec<u16> = [2u16, u16::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u16>::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32array() -> Result<(), Box<dyn Error>> {
        let extract_value: Vec<u32> = [2u32, u32::MAX].into();
        let data = extract_value.encode();
        let info = Vec::<u32>::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple() -> Result<(), Box<dyn Error>> {
        let extract_value: (i128, Vec<String>) = (i128::MIN, vec!["extract_value".into()]);
        let data = extract_value.encode();
        let info = <(i128, Vec<String>)>::type_info();
        let val = Value::new(&data, &info);
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
        let val = Value::new(&data, &info);

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
        let val = Value::new(&data, &info);

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
        let val = Value::new(&data, &info);

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
        let val = Value::new(&data, &info);

        assert_eq!(to_value(val)?, to_value(extract_value)?);
        Ok(())
    }
}
