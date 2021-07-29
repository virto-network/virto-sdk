use std::any::Any;

use scale_info::{Type, TypeDef};
use serde::ser::{Error, SerializeStruct};
use serde::{Serialize, Serializer};

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
            TypeDef::Primitive(u32) => ser.serialize_u32(self.data[0].into()),
            TypeDef::Composite(x) => {
                let fields = x.fields();
                let mut state = ser.serialize_struct("", fields.len())?;
                for (i, f) in fields.iter().enumerate() {
                    println!("{:?}", f);
                    let name = f.name().unwrap();
                    state.serialize_field(name, &self.data[i])?;
                }
                state.end()
            }
            _ => ser.serialize_bytes(self.data),
        }
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
    fn serialize_u32() -> Result<(), Box<dyn Error>> {
        let foo = 2u32;
        let data = foo.encode();
        let info = u32::type_info();
        let val = Value::new(&data, &info);
        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_struct() -> Result<(), Box<dyn Error>> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u32,
            baz: bool,
        }
        let foo = Foo {
            bar: 123,
            baz: true,
        };
        let data = foo.encode();
        let info = Foo::type_info();
        let val = Value::new(&data, &info);

        assert_eq!(to_value(val)?, to_value(foo)?);
        Ok(())
    }
}
