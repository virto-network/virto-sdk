use std::convert::TryInto;
use std::str;

use scale_info::form::MetaForm;
use scale_info::{Type, TypeDef, TypeDefPrimitive};
use serde::de::Visitor;
use serde::ser::SerializeSeq;
use serde::ser::SerializeTuple;
use serde::ser::{Error, SerializeStruct};
use serde::{Deserialize, Serialize, Serializer};

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
        match self.info.type_def() {
            TypeDef::Primitive(primitive) => match primitive {
                scale_info::TypeDefPrimitive::U8 => ser.serialize_u8(self.data[0].into()),
                scale_info::TypeDefPrimitive::U16 => ser.serialize_u16(self.data[0].into()),
                scale_info::TypeDefPrimitive::U32 => ser.serialize_u32(self.data[0].into()),
                scale_info::TypeDefPrimitive::U64 => ser.serialize_u64(self.data[0].into()),
                scale_info::TypeDefPrimitive::U128 => ser.serialize_u128(self.data[0].into()),
                scale_info::TypeDefPrimitive::I8 => {
                    ser.serialize_i8(self.data[0].try_into().unwrap())
                }
                scale_info::TypeDefPrimitive::I16 => {
                    ser.serialize_i16(self.data[0].try_into().unwrap())
                }
                scale_info::TypeDefPrimitive::I32 => {
                    ser.serialize_i32(self.data[0].try_into().unwrap())
                }
                scale_info::TypeDefPrimitive::I64 => {
                    ser.serialize_i64(self.data[0].try_into().unwrap())
                }
                scale_info::TypeDefPrimitive::I128 => {
                    ser.serialize_i128(self.data[0].try_into().unwrap())
                }
                scale_info::TypeDefPrimitive::Bool => ser.serialize_bool(self.data[0] != 0),
                scale_info::TypeDefPrimitive::Char => ser.serialize_char(self.data[0].into()),
                scale_info::TypeDefPrimitive::Str => {
                    ser.serialize_str(str::from_utf8(self.data).unwrap())
                }
                _ => ser.serialize_bytes(self.data),
            },
            TypeDef::Composite(x) => {
                let fields = x.fields();
                let mut state = ser.serialize_struct("", fields.len())?;
                let mut i = 0;
                for (_, f) in fields.iter().enumerate() {
                    let name = f.name().unwrap();
                    let t = f.ty().type_info();
                    let size = get_size(t.type_def());
                    state.serialize_field(name, &self.data[i])?;
                    i = i + size;
                }
                state.end()
            }
            TypeDef::Variant(_y) => ser.serialize_bytes(self.data),
            TypeDef::Sequence(_seq) => {
                let mut seq = ser.serialize_seq(Some(self.data.len()))?;
                println!("{:?}", self.data);
                for e in self.data {
                    seq.serialize_element(e)?;
                }
                seq.end()
            }
            TypeDef::Array(_) => {
                let mut seq = ser.serialize_seq(Some(self.data.len()))?;
                for e in self.data {
                    seq.serialize_element(e)?;
                }
                seq.end()
            }
            TypeDef::Tuple(x) => {
                let mut seq = ser.serialize_tuple(x.fields().len())?;
                let mut i = 0;
                for (_, f) in x.fields().iter().enumerate() {
                    let size = get_size(f.type_info().type_def());
                    seq.serialize_element(&self.data[i])?;
                    i = i + size;
                }
                seq.end()
            }
            TypeDef::Compact(_) => self.data.serialize(ser),
            TypeDef::Phantom(_) => self.data.serialize(ser),
        }
    }
}

fn get_size(t: &TypeDef<MetaForm>) -> usize {
    match t {
        TypeDef::Primitive(primitive) => match primitive {
            scale_info::TypeDefPrimitive::U8 => std::mem::size_of::<u8>(),
            scale_info::TypeDefPrimitive::U16 => std::mem::size_of::<u16>(),
            scale_info::TypeDefPrimitive::U32 => std::mem::size_of::<u32>(),
            scale_info::TypeDefPrimitive::U64 => std::mem::size_of::<u64>(),
            scale_info::TypeDefPrimitive::U128 => std::mem::size_of::<u128>(),
            scale_info::TypeDefPrimitive::I8 => std::mem::size_of::<i8>(),
            scale_info::TypeDefPrimitive::I16 => std::mem::size_of::<i16>(),
            scale_info::TypeDefPrimitive::I32 => std::mem::size_of::<i32>(),
            scale_info::TypeDefPrimitive::I64 => std::mem::size_of::<i64>(),
            scale_info::TypeDefPrimitive::I128 => std::mem::size_of::<i128>(),
            scale_info::TypeDefPrimitive::Bool => std::mem::size_of::<bool>(),
            scale_info::TypeDefPrimitive::Char => std::mem::size_of::<char>(),
            _ => 0,
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

impl<'de> Deserialize<'de> for Value<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ValueVisitor;

        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value<'de>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            todo!()
            }
        }

        deserializer.deserialize_u32(ValueVisitor)
    
    }

    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Default implementation just delegates to `deserialize` impl.
        *place = Deserialize::deserialize(deserializer)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;
    use parity_scale_codec::Encode;
    use scale_info::TypeInfo;
    use serde_json::to_value;
    use serde_json::from_slice;

    #[test]
    fn serialize_u32() -> Result<(), Box<dyn Error>> {
        let foo = 2u32;
        let data = foo.encode();
        let info = u32::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u64() -> Result<(), Box<dyn Error>> {
        let foo = 2u64;
        let data = foo.encode();
        let info = u64::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u16() -> Result<(), Box<dyn Error>> {
        let foo = 2u16;
        let data = foo.encode();
        let info = u16::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_u8() -> Result<(), Box<dyn Error>> {
        let foo = 2u8;
        let data = foo.encode();
        let info = u8::type_info();
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

    // #[test]
    // fn serialize_array() -> Result<(), Box<dyn Error>> {
    //     let foo: Vec<u16> = [2u16, 7u16].into();
    //     let data = foo.encode();
    //     let info = Vec::<u16>::type_info();
    //     let val = Value::new(&data, &info);
    //     assert_eq!(to_value(val)?, to_value(foo)?);
    //     Ok(())
    // }

    #[test]
    fn serialize_tuple() -> Result<(), Box<dyn Error>> {
        let foo = (2u8, 2u8);
        let data = foo.encode();
        let info = <(u8, u8)>::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_struct() -> Result<(), Box<dyn Error>> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u32,
            baz: u32,
        }
        let foo = Foo { bar: 123, baz: 45 };
        let data = foo.encode();
        let info = Foo::type_info();
        let val = Value::new(&data, &info);

        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn deserialize_u32() -> Result<(), Box<dyn Error>> {
        let foo = 2u32;
        let data = foo.encode();
        println!("{:?}", data);
        let info = u32::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(from_slice::<u32>(val.data)?, from_slice::<u32>(&data)?);
        Ok(())
    }
}
