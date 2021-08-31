use byteorder::{ByteOrder, LE};
use scale_info::form::MetaForm;
use scale_info::{prelude::*, TypeDefPrimitive};
use scale_info::{Type, TypeDef};
use serde::ser::SerializeSeq;
use serde::ser::{SerializeStruct, SerializeTuple};
use serde::Serialize;
use std::str;

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
        let def = self.info.type_def();
        let size = size_of(def);
        let data = self.data;

        match def {
            TypeDef::Primitive(primitive) => match primitive {
                TypeDefPrimitive::U8 => ser.serialize_u8(data[0]),
                TypeDefPrimitive::U16 => ser.serialize_u16(LE::read_u16(&data[..size])),
                TypeDefPrimitive::U32 => ser.serialize_u32(LE::read_u32(&data[..size])),
                TypeDefPrimitive::U64 => ser.serialize_u64(LE::read_u64(&data[..size])),
                TypeDefPrimitive::U128 => ser.serialize_u128(LE::read_u128(&data[..size])),
                TypeDefPrimitive::I8 => ser.serialize_i8(i8::from_le_bytes([data[0]])),
                TypeDefPrimitive::I16 => ser.serialize_i16(LE::read_i16(&data[..size])),
                TypeDefPrimitive::I32 => ser.serialize_i32(LE::read_i32(&data[..size])),
                TypeDefPrimitive::I64 => ser.serialize_i64(LE::read_i64(&data[..size])),
                TypeDefPrimitive::I128 => ser.serialize_i128(LE::read_i128(&data[..size])),
                TypeDefPrimitive::Bool => ser.serialize_bool(data[0] != 0),
                TypeDefPrimitive::Char => {
                    let n = LE::read_u32(&data[..size]);
                    ser.serialize_char(char::from_u32(n).unwrap())
                }
                TypeDefPrimitive::Str => ser.serialize_str(str::from_utf8(self.data).unwrap()),
                _ => ser.serialize_bytes(self.data),
            },
            TypeDef::Composite(x) => {
                let fields = x.fields();
                let mut state = ser.serialize_struct("", fields.len())?;
                let mut i = 0;
                for (_, f) in fields.iter().enumerate() {
                    let name = f.name().unwrap();
                    let t = f.ty().type_info();
                    match t.type_def() {
                        TypeDef::Primitive(_) => {
                            let size = size_of(t.type_def());
                            state.serialize_field(name, &self.data[i])?;
                            i += size;
                        }
                        _ => {
                            println!("{:?}", t.type_def());
                            println!("{:?}", self.data);
                            println!("{:?}", f.to_owned());
                            let size = std::mem::size_of_val(&f.to_owned().ty());
                            let u = &f.ty().type_info();
                            let data = Value::new(&self.data, u);
                            println!("{:?}", size);
                            state.serialize_field(name, &data)?;
                            i += size;
                        }
                        // TypeDef::Composite(_) => todo!(),
                        // TypeDef::Variant(_) => todo!(),
                        // TypeDef::Sequence(_) => todo!(),
                        // TypeDef::Array(_) => todo!(),
                        // TypeDef::Tuple(_) => todo!(),
                        // TypeDef::Compact(_) => todo!(),
                        // TypeDef::Phantom(_) => todo!(),
                    }
                }
                state.end()
            }
            TypeDef::Variant(_y) => ser.serialize_bytes(self.data),
            TypeDef::Sequence(x) => {
                let size = size_of(x.type_param().type_info().type_def());
                let mut seq = ser.serialize_seq(Some(self.data.len()))?;
                let mut i: usize = 1;
                while i < self.data.len() {
                    seq.serialize_element(&self.data[i])?;
                    i += size;
                }
                seq.end()
            }
            TypeDef::Array(x) => {
                let size = size_of(x.type_param().type_info().type_def());
                let mut seq = ser.serialize_seq(Some(self.data.len()))?;
                let mut i: usize = 1;
                while i < self.data.len() {
                    seq.serialize_element(&self.data[i])?;
                    i += size;
                }
                seq.end()
            }
            TypeDef::Tuple(x) => {
                let mut seq = ser.serialize_tuple(x.fields().len())?;
                let mut i = 0;
                for (_, f) in x.fields().iter().enumerate() {
                    let size = size_of(f.type_info().type_def());
                    seq.serialize_element(&self.data[i])?;
                    i += size;
                }
                seq.end()
            }
            TypeDef::Compact(_) => self.data.serialize(ser),
            TypeDef::Phantom(_) => self.data.serialize(ser),
        }
    }
}

fn size_of(t: &TypeDef<MetaForm>) -> usize {
    match t {
        TypeDef::Primitive(primitive) => match primitive {
            scale_info::TypeDefPrimitive::U8 => mem::size_of::<u8>(),
            scale_info::TypeDefPrimitive::U16 => mem::size_of::<u16>(),
            scale_info::TypeDefPrimitive::U32 => mem::size_of::<u32>(),
            scale_info::TypeDefPrimitive::U64 => mem::size_of::<u64>(),
            scale_info::TypeDefPrimitive::U128 => mem::size_of::<u128>(),
            scale_info::TypeDefPrimitive::I8 => mem::size_of::<i8>(),
            scale_info::TypeDefPrimitive::I16 => mem::size_of::<i16>(),
            scale_info::TypeDefPrimitive::I32 => mem::size_of::<i32>(),
            scale_info::TypeDefPrimitive::I64 => mem::size_of::<i64>(),
            scale_info::TypeDefPrimitive::I128 => mem::size_of::<i128>(),
            scale_info::TypeDefPrimitive::Bool => mem::size_of::<bool>(),
            scale_info::TypeDefPrimitive::Char => mem::size_of::<char>(),
            _ => unimplemented!(),
        },
        TypeDef::Composite(_) => todo!(),
        TypeDef::Variant(_) => todo!(),
        TypeDef::Sequence(_) => todo!(),
        TypeDef::Array(_) => todo!(),
        TypeDef::Tuple(_) => todo!(),
        TypeDef::Compact(_) => todo!(),
        TypeDef::Phantom(_) => todo!(),
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
        let foo = u8::MAX;
        let data = foo.encode();
        let info = u8::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u16() -> Result<(), Box<dyn Error>> {
        let foo = u16::MAX;
        let data = foo.encode();
        let info = u16::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u32() -> Result<(), Box<dyn Error>> {
        let foo = u32::MAX;
        let data = foo.encode();
        let info = u32::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u64() -> Result<(), Box<dyn Error>> {
        let foo = u64::MAX;
        let data = foo.encode();
        let info = u64::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_i16() -> Result<(), Box<dyn Error>> {
        let foo = i16::MAX;
        let data = foo.encode();
        let info = i16::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_i32() -> Result<(), Box<dyn Error>> {
        let foo = i32::MAX;
        let data = foo.encode();
        let info = i32::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_i64() -> Result<(), Box<dyn Error>> {
        let foo = i64::MAX;
        let data = foo.encode();
        let info = i64::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_bool() -> Result<(), Box<dyn Error>> {
        let foo = true;
        let data = foo.encode();
        let info = bool::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u8array() -> Result<(), Box<dyn Error>> {
        let foo: Vec<u8> = [2u8, u8::MAX].into();
        let data = foo.encode();
        let info = Vec::<u8>::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u16array() -> Result<(), Box<dyn Error>> {
        let foo: Vec<u16> = [2u16, u16::MAX].into();
        let data = foo.encode();
        let info = Vec::<u16>::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u32array() -> Result<(), Box<dyn Error>> {
        let foo: Vec<u32> = [2u32, u32::MAX].into();
        let data = foo.encode();
        let info = Vec::<u32>::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple() -> Result<(), Box<dyn Error>> {
        let foo: (i128, Vec<String>) = (i128::MIN, vec!["foo".into()]);
        let data = foo.encode();
        let info = <(i128, Vec<String>)>::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u32struct() -> Result<(), Box<dyn Error>> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u32,
            baz: u32,
        }
        let foo = Foo {
            bar: 123,
            baz: u32::MAX,
        };
        let data = foo.encode();
        let info = Foo::type_info();
        let val = Value::new(&data, &info);

        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u8struct() -> Result<(), Box<dyn Error>> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u8,
            baz: u8,
        }
        let foo = Foo {
            bar: 123,
            baz: u8::MAX,
        };
        let data = foo.encode();
        let info = Foo::type_info();
        let val = Value::new(&data, &info);

        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u64struct() -> Result<(), Box<dyn Error>> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u64,
            baz: u64,
        }
        let foo = Foo {
            bar: 123,
            baz: u64::MAX,
        };
        let data = foo.encode();
        let info = Foo::type_info();
        let val = Value::new(&data, &info);

        assert_eq!(to_value(val)?, to_value(foo)?);
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
        let foo = Foo {
            bar: [Bar::That(i16::MAX), Bar::This].into(),
            baz: Some("aliquam malesuada bibendum arcu vitae".into()),
        };
        let data = foo.encode();
        let info = Foo::type_info();
        let val = Value::new(&data, &info);

        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }
}
