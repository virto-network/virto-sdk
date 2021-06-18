use scale_info::Type;
use serde::Serialize;

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
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        todo!()
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
