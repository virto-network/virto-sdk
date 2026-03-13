use alloc::collections::BTreeMap;

use super::register;
use crate::registry::*;
use crate::value::{sequence_size, Value};
use anyhow::Error;
use codec::Encode;
use scale_info::TypeInfo;
use serde_json::to_value;

#[test]
fn test_compact_two_bytes() {
    let data: [u8; 2] = [0x99, 0x01];
    assert_eq!(sequence_size(&data).unwrap(), (102, 2));

    let data: [u8; 2] = [0x15, 0x01];
    assert_eq!(sequence_size(&data).unwrap(), (69, 2));

    let data: [u8; 4] = [0xfe, 0xff, 0x03, 0x00];
    assert_eq!(sequence_size(&data).unwrap(), (65535, 4));
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
    let out_value = Value::new(&data, id, &reg).to_string();

    #[cfg(feature = "text")]
    assert_eq!("(bar:'BAZ')", out_value);
    #[cfg(not(feature = "text"))]
    assert_eq!("{\"bar\":\"BAZ\"}", out_value);
}

#[test]
fn serialize_u8() -> Result<(), Error> {
    let in_value = u8::MAX;
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_u16() -> Result<(), Error> {
    let in_value = u16::MAX;
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_u32() -> Result<(), Error> {
    let in_value = u32::MAX;
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_u64() -> Result<(), Error> {
    let in_value = u64::MAX;
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_bool() -> Result<(), Error> {
    let in_value = true;
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_i16() -> Result<(), Error> {
    let in_value = i16::MAX;
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_i32() -> Result<(), Error> {
    let in_value = i32::MAX;
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_i64() -> Result<(), Error> {
    let in_value = i64::MAX;
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_tuple() -> Result<(), Error> {
    let in_value = (u8::MAX, i8::MIN, true);
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_tuple_struct() -> Result<(), Error> {
    #[derive(Encode, TypeInfo, serde::Serialize)]
    struct Baz(String, u16);

    let in_value = Baz("lol".into(), u16::MAX);
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_u8array() -> Result<(), Error> {
    let in_value: Vec<u8> = vec![0, 1, 2, 3, 4, 5];
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(&out_value)?, to_value(in_value.as_slice())?);
    Ok(())
}

#[test]
fn serialize_u16array() -> Result<(), Error> {
    let in_value: Vec<u16> = vec![0, 1, 2, 3, 4, 5];
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_u32array() -> Result<(), Error> {
    let in_value: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_simple_u8struct() -> Result<(), Error> {
    #[derive(Encode, TypeInfo, serde::Serialize)]
    struct Bar(u8);

    let in_value = Bar(0xFF);
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_simple_u32struct() -> Result<(), Error> {
    #[derive(Encode, TypeInfo, serde::Serialize)]
    struct Bar(u32);

    let in_value = Bar(0xFF_EE_DD_CC);
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_simple_u64struct() -> Result<(), Error> {
    #[derive(Encode, TypeInfo, serde::Serialize)]
    struct Bar(u64);

    let in_value = Bar(0xFFEE_DDCC_BBAA_9988);
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_complex_struct_with_enum() -> Result<(), Error> {
    #[derive(Encode, TypeInfo, serde::Serialize)]
    struct Foo {
        a: Bar,
        b: Option<Baz>,
    }
    #[derive(Encode, TypeInfo, serde::Serialize)]
    struct Bar(u8);
    #[derive(Encode, TypeInfo, serde::Serialize)]
    struct Baz(String, u16);

    let in_value = Foo {
        a: Bar(0xFF),
        b: Some(Baz("lol".into(), u16::MAX)),
    };
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn serialize_map() -> Result<(), Error> {
    let mut in_value = BTreeMap::<String, u32>::new();
    in_value.insert("key".into(), 1234);
    let data = in_value.encode();
    let (id, reg) = register(&in_value);

    let out_value = Value::new(&data, id, &reg);

    assert_eq!(to_value(out_value)?, to_value(in_value)?);
    Ok(())
}

#[test]
fn test_primitive_extraction() {
    let (id, reg) = register(&42u32);
    let data = 42u32.encode();
    let value = Value::new(&data, id, &reg);

    assert!(value.is_u32());
    assert_eq!(value.as_u32(), Some(42));
    assert_eq!(value.as_u64(), None);
}

#[test]
fn test_string_extraction() {
    let (id, reg) = register(&"hello");
    let data = "hello".encode();
    let value = Value::new(&data, id, &reg);

    assert!(value.is_string());
    assert_eq!(value.as_str(), Some("hello"));
}

#[test]
fn test_composite_field_access() {
    #[derive(Encode, TypeInfo)]
    struct Foo {
        bar: u32,
        baz: String,
    }

    let foo = Foo {
        bar: 42,
        baz: "hello".into(),
    };
    let data = foo.encode();
    let (id, reg) = register(&foo);
    let value = Value::new(&data, id, &reg);

    assert!(value.is_composite());
    assert_eq!(value.field_count(), Some(2));

    let bar = value.field("bar").unwrap();
    assert_eq!(bar.as_u32(), Some(42));

    let baz = value.field("baz").unwrap();
    assert_eq!(baz.as_str(), Some("hello"));

    let bar_by_index = value.field_at(0).unwrap();
    assert_eq!(bar_by_index.as_u32(), Some(42));
}

#[test]
fn test_sequence_operations() {
    let input: Vec<u32> = vec![1, 2, 3, 4, 5];
    let data = input.encode();
    let (id, reg) = register(&input);
    let value = Value::new(&data, id, &reg);

    assert!(value.is_sequence());
    assert_eq!(value.sequence_len(), Some(5));

    let first = value.sequence_get(0).unwrap();
    assert_eq!(first.as_u32(), Some(1));

    let third = value.sequence_get(2).unwrap();
    assert_eq!(third.as_u32(), Some(3));

    assert!(value.sequence_get(5).is_none());
}

#[test]
fn test_tuple_operations() {
    let input: (u32, bool, u8) = (42, true, 7);
    let data = input.encode();
    let (id, reg) = register(&input);
    let value = Value::new(&data, id, &reg);

    assert!(value.is_tuple());
    assert_eq!(value.tuple_len(), Some(3));

    let first = value.tuple_get(0).unwrap();
    assert_eq!(first.as_u32(), Some(42));

    let second = value.tuple_get(1).unwrap();
    assert_eq!(second.as_bool(), Some(true));

    let third = value.tuple_get(2).unwrap();
    assert_eq!(third.as_u8(), Some(7));

    assert!(value.tuple_get(3).is_none());
}

#[test]
fn test_variant_operations() {
    #[derive(Encode, TypeInfo)]
    enum MyEnum {
        UnitVariant,
        #[allow(dead_code)]
        DataVariant(u32),
    }

    let input = MyEnum::UnitVariant;
    let data = input.encode();
    let (id, reg) = register(&input);
    let value = Value::new(&data, id, &reg);

    assert!(value.is_variant());
    assert_eq!(value.variant_index(), Some(0));
    assert_eq!(value.variant_name(), Some("UnitVariant"));
    assert!(value.variant_data().is_none());

    let input = MyEnum::DataVariant(42);
    let data = input.encode();
    let value = Value::new(&data, id, &reg);

    assert_eq!(value.variant_index(), Some(1));
    assert_eq!(value.variant_name(), Some("DataVariant"));
    let inner = value.variant_data().unwrap();
    assert_eq!(inner.as_u32(), Some(42));
}

#[test]
fn test_all_unsigned_extraction() {
    let (id, reg) = register(&0u8);
    let data = 255u8.encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_u8());
    assert_eq!(v.as_u8(), Some(255));

    let (id, reg) = register(&0u16);
    let data = 0xABCDu16.encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_u16());
    assert_eq!(v.as_u16(), Some(0xABCD));

    let (id, reg) = register(&0u64);
    let data = u64::MAX.encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_u64());
    assert_eq!(v.as_u64(), Some(u64::MAX));

    let (id, reg) = register(&0u128);
    let data = u128::MAX.encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_u128());
    assert_eq!(v.as_u128(), Some(u128::MAX));
}

#[test]
fn test_all_signed_extraction() {
    let (id, reg) = register(&0i8);
    let data = (-42i8).encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_i8());
    assert_eq!(v.as_i8(), Some(-42));

    let (id, reg) = register(&0i16);
    let data = i16::MIN.encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_i16());
    assert_eq!(v.as_i16(), Some(i16::MIN));

    let (id, reg) = register(&0i32);
    let data = i32::MIN.encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_i32());
    assert_eq!(v.as_i32(), Some(i32::MIN));

    let (id, reg) = register(&0i64);
    let data = i64::MIN.encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_i64());
    assert_eq!(v.as_i64(), Some(i64::MIN));

    let (id, reg) = register(&0i128);
    let data = i128::MIN.encode();
    let v = Value::new(&data, id, &reg);
    assert!(v.is_i128());
    assert_eq!(v.as_i128(), Some(i128::MIN));
}

#[test]
fn test_bool_extraction() {
    let (id, reg) = register(&true);
    let data = true.encode();
    assert!(Value::new(&data, id, &reg).is_bool());
    assert_eq!(Value::new(&data, id, &reg).as_bool(), Some(true));
    let data = false.encode();
    assert_eq!(Value::new(&data, id, &reg).as_bool(), Some(false));
}

#[test]
fn test_type_mismatch_returns_none() {
    let (uid, reg) = register(&0u32);
    let data = 42u32.encode();
    let v = Value::new(&data, uid, &reg);

    assert_eq!(v.as_u8(), None);
    assert_eq!(v.as_u16(), None);
    assert_eq!(v.as_u64(), None);
    assert_eq!(v.as_i32(), None);
    assert_eq!(v.as_str(), None);
    assert_eq!(v.as_bool(), None);
    assert!(!v.is_string());
    assert!(!v.is_bool());
    assert!(!v.is_variant());
    assert!(!v.is_sequence());
    assert!(!v.is_tuple());
    assert!(!v.is_composite());
}

#[test]
fn test_truncated_data_returns_none() {
    let (id, reg) = register(&0u32);
    // only 2 bytes for a u32
    let v = Value::new(&[0u8, 0], id, &reg);
    assert_eq!(v.as_u32(), None);

    let (id, reg) = register(&0u128);
    let v = Value::new(&[0u8; 8], id, &reg);
    assert_eq!(v.as_u128(), None);

    // empty data
    let (id, reg) = register(&0u8);
    let v = Value::new(&[], id, &reg);
    assert_eq!(v.as_u8(), None);
}

#[test]
fn test_field_access_nonexistent() {
    #[derive(Encode, TypeInfo)]
    struct Foo {
        bar: u32,
    }
    let foo = Foo { bar: 1 };
    let (id, reg) = register(&foo);
    let data = foo.encode();
    let v = Value::new(&data, id, &reg);

    assert!(v.field("nonexistent").is_none());
    assert!(v.field_at(1).is_none());
    assert!(v.field_at(100).is_none());
}

#[test]
fn test_field_on_non_struct() {
    let (id, reg) = register(&42u32);
    let data = 42u32.encode();
    let v = Value::new(&data, id, &reg);

    assert!(v.field("x").is_none());
    assert!(v.field_at(0).is_none());
    assert_eq!(v.field_count(), None);
}

#[test]
fn test_sequence_on_non_sequence() {
    let (id, reg) = register(&42u32);
    let data = 42u32.encode();
    let v = Value::new(&data, id, &reg);

    assert_eq!(v.sequence_len(), None);
    assert!(v.sequence_get(0).is_none());
}

#[test]
fn test_tuple_on_non_tuple() {
    let (id, reg) = register(&42u32);
    let data = 42u32.encode();
    let v = Value::new(&data, id, &reg);

    assert_eq!(v.tuple_len(), None);
    assert!(v.tuple_get(0).is_none());
}

#[test]
fn test_variant_on_non_variant() {
    let (id, reg) = register(&42u32);
    let data = 42u32.encode();
    let v = Value::new(&data, id, &reg);

    assert_eq!(v.variant_index(), None);
    assert_eq!(v.variant_name(), None);
    assert!(v.variant_data().is_none());
}

#[test]
fn test_empty_sequence() {
    let input: Vec<u32> = vec![];
    let data = input.encode();
    let (id, reg) = register(&input);
    let v = Value::new(&data, id, &reg);

    assert_eq!(v.sequence_len(), Some(0));
    assert!(v.sequence_get(0).is_none());
}

#[test]
fn test_array_operations() {
    let input: [u16; 3] = [10, 20, 30];
    let data = input.encode();
    let (id, reg) = register(&input);
    let v = Value::new(&data, id, &reg);

    assert!(v.is_array());
    assert_eq!(v.array_len(), Some(3));
    assert_eq!(v.array_get(0).unwrap().as_u16(), Some(10));
    assert_eq!(v.array_get(2).unwrap().as_u16(), Some(30));
    assert!(v.array_get(3).is_none());
}

#[test]
fn test_compact_value() {
    use codec::Compact;
    let input = Compact(42u32);
    let data = input.encode();
    let (id, reg) = register(&input);
    let v = Value::new(&data, id, &reg);

    assert!(v.is_compact());
}

#[test]
fn test_option_none() -> Result<(), Error> {
    let input: Option<u32> = None;
    let data = input.encode();
    let (id, reg) = register(&input);
    let v = Value::new(&data, id, &reg);

    assert!(v.is_variant());
    assert_eq!(v.variant_name(), Some("None"));
    assert_eq!(to_value(v)?, serde_json::Value::Null);
    Ok(())
}

#[test]
fn test_option_some() -> Result<(), Error> {
    let input: Option<u32> = Some(42);
    let data = input.encode();
    let (id, reg) = register(&input);
    let v = Value::new(&data, id, &reg);

    assert!(v.is_variant());
    assert_eq!(v.variant_name(), Some("Some"));
    assert_eq!(to_value(v)?, to_value(42u32)?);
    Ok(())
}

#[test]
fn test_variant_with_tuple_fields() -> Result<(), Error> {
    #[derive(Encode, TypeInfo, serde::Serialize)]
    enum Msg {
        #[allow(dead_code)]
        A,
        B(u32, String),
    }
    let input = Msg::B(7, "hi".into());
    let data = input.encode();
    let (id, reg) = register(&input);
    let v = Value::new(&data, id, &reg);

    assert_eq!(v.variant_name(), Some("B"));
    // tuple variant data is not accessible via variant_data (only NewType)
    assert!(v.variant_data().is_none());
    // but full serialization should still work
    assert_eq!(to_value(v)?, to_value(&input)?);
    Ok(())
}

#[test]
fn test_variant_with_struct_fields() -> Result<(), Error> {
    #[derive(Encode, TypeInfo, serde::Serialize)]
    enum Msg {
        #[allow(dead_code)]
        A,
        B {
            x: u32,
            y: String,
        },
    }
    let input = Msg::B {
        x: 99,
        y: "hello".into(),
    };
    let data = input.encode();
    let (id, reg) = register(&input);
    let v = Value::new(&data, id, &reg);

    assert_eq!(v.variant_name(), Some("B"));
    assert_eq!(to_value(v)?, to_value(&input)?);
    Ok(())
}

#[test]
fn test_map_operations() -> Result<(), Error> {
    let mut input = BTreeMap::<String, u32>::new();
    input.insert("a".into(), 1);
    input.insert("b".into(), 2);
    input.insert("c".into(), 3);
    let data = input.encode();
    let (id, reg) = register(&input);
    let v = Value::new(&data, id, &reg);

    assert_eq!(to_value(v)?, to_value(&input)?);
    Ok(())
}

#[test]
fn test_empty_struct() {
    #[derive(Encode, TypeInfo)]
    struct Unit;
    let data = Unit.encode();
    let (id, reg) = register(&Unit);
    let v = Value::new(&data, id, &reg);

    // StructUnit is not composite (no fields)
    assert_eq!(v.field_count(), None);
}

#[test]
fn test_size_calculation() {
    // primitives
    let (id, reg) = register(&0u8);
    let data = 0u8.encode();
    assert_eq!(Value::new(&data, id, &reg).size().unwrap(), 1);

    let (id, reg) = register(&0u32);
    let data = 0u32.encode();
    assert_eq!(Value::new(&data, id, &reg).size().unwrap(), 4);

    let (id, reg) = register(&0u128);
    let data = 0u128.encode();
    assert_eq!(Value::new(&data, id, &reg).size().unwrap(), 16);

    // string
    let (id, reg) = register(&"hello");
    let data = "hello".encode();
    assert_eq!(Value::new(&data, id, &reg).size().unwrap(), 1 + 5); // compact(5) + 5 bytes

    // sequence
    let input: Vec<u32> = vec![1, 2, 3];
    let (id, reg) = register(&input);
    let data = input.encode();
    assert_eq!(Value::new(&data, id, &reg).size().unwrap(), 1 + 3 * 4); // compact(3) + 3*4
}

#[test]
fn test_registry_resolve_invalid_id() {
    let reg = Registry::new(vec![TypeDef::U8]);
    assert!(reg.resolve(0).is_some());
    assert!(reg.resolve(1).is_none());
    assert!(reg.resolve(u32::MAX).is_none());
}
