use crate::{EnumVariant, SpecificType};
use alloc::{collections::BTreeMap, vec::Vec};
use bytes::{Buf, Bytes};
use codec::Encode;
use core::{convert::TryInto, str};
use scale_info::{prelude::*, PortableRegistry, TypeDefPrimitive as Primitive};
use serde::ser::{SerializeMap, SerializeSeq, SerializeTuple, SerializeTupleStruct};
use serde::Serialize;

type Type = scale_info::Type<scale_info::form::PortableForm>;
type TypeId = u32;
type TypeDef = scale_info::TypeDef<scale_info::form::PortableForm>;

/// A container for SCALE encoded data that can serialize types directly
/// with the help of a type registry and without using an intermediate representation.
pub struct Value<'a> {
    data: Bytes,
    ty_id: TypeId,
    registry: &'a PortableRegistry,
}

impl<'a> Value<'a> {
    pub fn new(data: impl Into<Bytes>, ty_id: u32, registry: &'a PortableRegistry) -> Self {
        Value {
            data: data.into(),
            ty_id,
            registry,
        }
    }

    fn new_value(&self, data: &mut Bytes, ty_id: TypeId) -> Self {
        let size = self.ty_len(data.chunk(), ty_id);
        Value::new(data.copy_to_bytes(size), ty_id, self.registry)
    }

    #[inline]
    fn resolve(&self, ty: TypeId) -> &'a Type {
        self.registry.resolve(ty).expect("in registry")
    }

    fn ty_len(&self, data: &[u8], ty: TypeId) -> usize {
        match self.resolve(ty).type_def() {
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
                .fold(0, |c, f| c + self.ty_len(&data[c..], f.ty().id())),
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
                        .fold(1, |c, f| c + self.ty_len(&data[c..], f.ty().id()))
                }
            }
            TypeDef::Sequence(s) => {
                let (len, prefix_size) = sequence_len(data);
                let ty_id = s.type_param().id();
                (0..len).fold(prefix_size, |c, _| c + self.ty_len(&data[c..], ty_id))
            }
            TypeDef::Array(a) => a.len().try_into().unwrap(),
            TypeDef::Tuple(t) => t.fields().len(),
            TypeDef::Compact(_) => compact_len(data),
            TypeDef::BitSequence(_) => unimplemented!(),
        }
    }
}

impl<'a> Serialize for Value<'a> {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut data = self.data.clone();
        let ty = self.resolve(self.ty_id);

        use SpecificType::*;
        match (ty, self.registry).into() {
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
            Compact(ty) => {
                let type_def = self
                    .registry
                    .resolve(ty)
                    .expect("not found in registry")
                    .type_def();

                use codec::Compact;
                match type_def {
                    TypeDef::Primitive(Primitive::U32) => {
                        ser.serialize_bytes(&Compact(data.get_u32_le()).encode())
                    }
                    TypeDef::Primitive(Primitive::U64) => {
                        ser.serialize_bytes(&Compact(data.get_u64_le()).encode())
                    }
                    TypeDef::Primitive(Primitive::U128) => {
                        ser.serialize_bytes(&Compact(data.get_u128_le()).encode())
                    }
                    _ => unimplemented!(),
                }
            }
            Bytes(_) => {
                let (_, s) = sequence_len(data.chunk());
                data.advance(s);
                ser.serialize_bytes(data.chunk())
            }
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
                    seq.serialize_element(&self.new_value(&mut data, ty))?;
                }
                seq.end()
            }
            Map(ty_k, ty_v) => {
                let (len, p_size) = sequence_len(data.chunk());
                data.advance(p_size);

                let mut state = ser.serialize_map(Some(len))?;
                for _ in 0..len {
                    let key = self.new_value(&mut data, ty_k);
                    let val = self.new_value(&mut data, ty_v);
                    state.serialize_entry(&key, &val)?;
                }
                state.end()
            }
            Tuple(t) => {
                let mut state = ser.serialize_tuple(t.len())?;
                for i in 0..t.len() {
                    state.serialize_element(&self.new_value(&mut data, t.type_id(i)))?;
                }
                state.end()
            }
            Struct(fields) => {
                let mut state = ser.serialize_map(Some(fields.len()))?;
                for (name, ty) in fields {
                    state.serialize_key(&name)?;
                    state.serialize_value(&self.new_value(&mut data, ty))?;
                }
                state.end()
            }
            StructUnit => ser.serialize_unit(),
            StructNewType(ty) => ser.serialize_newtype_struct("", &self.new_value(&mut data, ty)),
            StructTuple(fields) => {
                let mut state = ser.serialize_tuple_struct("", fields.len())?;
                for ty in fields {
                    state.serialize_field(&self.new_value(&mut data, ty))?;
                }
                state.end()
            }
            ty @ Variant(_, _, _) => {
                let variant = &ty.pick(data.get_u8());
                match variant.into() {
                    EnumVariant::OptionNone => ser.serialize_none(),
                    EnumVariant::OptionSome(ty) => {
                        ser.serialize_some(&self.new_value(&mut data, ty))
                    }
                    EnumVariant::Unit(_idx, name) => ser.serialize_str(name),
                    EnumVariant::NewType(_idx, name, ty) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(name)?;
                        s.serialize_value(&self.new_value(&mut data, ty))?;
                        s.end()
                    }

                    EnumVariant::Tuple(_idx, name, fields) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(name)?;
                        s.serialize_value(
                            &fields
                                .iter()
                                .map(|ty| self.new_value(&mut data, *ty))
                                .collect::<Vec<_>>(),
                        )?;
                        s.end()
                    }
                    EnumVariant::Struct(_idx, name, fields) => {
                        let mut s = ser.serialize_map(Some(1))?;
                        s.serialize_key(name)?;
                        s.serialize_value(&fields.iter().fold(
                            BTreeMap::new(),
                            |mut m, (name, ty)| {
                                m.insert(*name, self.new_value(&mut data, *ty));
                                m
                            },
                        ))?;
                        s.end()
                    }
                }
            }
        }
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

impl<'reg> AsRef<[u8]> for Value<'reg> {
    fn as_ref(&self) -> &[u8] {
        self.data.as_ref()
    }
}

#[cfg(feature = "codec")]
impl<'reg> codec::Encode for Value<'reg> {
    fn size_hint(&self) -> usize {
        self.data.len()
    }
    fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
        f(self.data.as_ref())
    }
}

impl<'reg> core::fmt::Debug for Value<'reg> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Value {{ data: {:?}, type({}): {:?} }}",
            self.data,
            self.ty_id,
            self.registry.resolve(self.ty_id).unwrap().type_def()
        )
    }
}

#[cfg(feature = "json")]
impl<'reg> core::fmt::Display for Value<'reg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self).map_err(|_| fmt::Error)?
        )
    }
}

#[cfg(feature = "json")]
impl<'reg> From<Value<'reg>> for crate::JsonValue {
    fn from(val: Value<'reg>) -> Self {
        serde_json::value::to_value(val).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use super::*;
    use anyhow::Error;
    use codec::Encode;
    use scale_info::{
        meta_type,
        prelude::{string::String, vec::Vec},
        Registry, TypeInfo,
    };
    use serde_json::to_value;

    fn register<T>(_ty: &T) -> (u32, PortableRegistry)
    where
        T: TypeInfo + 'static,
    {
        let mut reg = Registry::new();
        let sym = reg.register_type(&meta_type::<T>());
        (sym.id(), reg.into())
    }

    #[cfg(feature = "json")]
    #[test]
    fn display_as_json() {
        #[derive(Encode, TypeInfo)]
        struct Foo {
            bar: String,
        }
        let in_value = Foo { bar: "BAZ".into() };

        let data = in_value.encode();
        let (id, reg) = register(&in_value);
        let out_value = Value::new(data, id, &reg).to_string();

        assert_eq!("{\"bar\":\"BAZ\"}", out_value);
    }

    #[cfg(feature = "codec")]
    #[test]
    fn encodable() {
        let input = u8::MAX;
        let (ty, reg) = register(&input);
        let value = Value::new(b"1234".as_ref(), ty, &reg);

        let expected: &[u8] = value.as_ref();
        assert_eq!(value.encode(), expected);
    }

    #[test]
    fn serialize_u8() -> Result<(), Error> {
        let in_value = u8::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16() -> Result<(), Error> {
        let in_value = u16::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32() -> Result<(), Error> {
        let in_value = u32::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u64() -> Result<(), Error> {
        let in_value = u64::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i16() -> Result<(), Error> {
        let in_value = i16::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i32() -> Result<(), Error> {
        let in_value = i32::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_i64() -> Result<(), Error> {
        let in_value = i64::MAX;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_bool() -> Result<(), Error> {
        let in_value = true;
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    // `char` not supported?
    // #[test]
    // fn serialize_char() -> Result<(), Error> {
    //     let extract_value = '⚖';
    //     let data = extract_value.encode();
    //     let info = char::type_info();
    //     let val = Value::new(data, info, reg);
    //     assert_eq!(to_value(val)?, to_value(extract_value)?);
    //     Ok(())
    // }

    #[test]
    fn serialize_u8array() -> Result<(), Error> {
        let in_value: Vec<u8> = [2u8; u8::MAX as usize].into();
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u16array() -> Result<(), Error> {
        let in_value: Vec<u16> = [2u16, u16::MAX].into();
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_u32array() -> Result<(), Error> {
        let in_value: Vec<u32> = [2u32, u32::MAX].into();
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple() -> Result<(), Error> {
        let in_value: (i64, Vec<String>, bool) = (
            i64::MIN,
            vec!["hello".into(), "big".into(), "world".into()],
            true,
        );
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u32struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u32,
            baz: u32,
        }
        let in_value = Foo {
            bar: 123,
            baz: u32::MAX,
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u8struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u8,
            baz: u8,
        }
        let in_value = Foo {
            bar: 123,
            baz: u8::MAX,
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_simple_u64struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo {
            bar: u64,
            baz: u64,
        }
        let in_value = Foo {
            bar: 123,
            baz: u64::MAX,
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_map() -> Result<(), Error> {
        let in_value = {
            let mut m = BTreeMap::<String, i32>::new();
            m.insert("foo".into(), i32::MAX);
            m.insert("bar".into(), i32::MIN);
            m
        };

        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
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
        let in_value = Foo {
            bar: [Bar::That(i16::MAX), Bar::This].into(),
            baz: Some(Baz("aliquam malesuada bibendum arcu vitae".into())),
            lol: b"\0xFFsome stuff\0x00",
        };
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }

    #[test]
    fn serialize_tuple_struct() -> Result<(), Error> {
        #[derive(Encode, Serialize, TypeInfo)]
        struct Foo([u8; 4], (bool, Option<()>), Baz, Baz);

        #[derive(Encode, Serialize, TypeInfo)]
        struct Bar;

        #[derive(Encode, Serialize, TypeInfo)]
        enum Baz {
            A(Bar),
            B { bb: &'static str },
        }

        let in_value = Foo(
            [1, 2, 3, 4],
            (false, None),
            Baz::A(Bar),
            Baz::B { bb: "lol" },
        );
        let data = in_value.encode();
        let (id, reg) = register(&in_value);

        let out_value = Value::new(data, id, &reg);

        assert_eq!(to_value(out_value)?, to_value(in_value)?);
        Ok(())
    }
}
