use alloc::collections::BTreeMap;

use super::register;
use crate::serializer::*;
use codec::{Decode, Encode};
use core::mem::size_of;
use scale_info::{PortableRegistry, TypeInfo};
use serde::Serialize;
use serde_json::to_value;

type Result<T> = core::result::Result<T, crate::Error>;

#[test]
fn primitive_u8() -> Result<()> {
    let mut out = [0u8];
    to_bytes(&mut out[..], &123u8)?;

    let expected = [123];

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn primitive_u16() -> Result<()> {
    const INPUT: u16 = 0xFF_EE;
    let mut out = [0u8; size_of::<u16>()];
    let expected = INPUT.encode();

    to_bytes(out.as_mut(), &INPUT)?;

    assert_eq!(out.as_ref(), expected);
    Ok(())
}

#[test]
fn primitive_u32() -> Result<()> {
    const INPUT: u32 = 0xFF_EE_DD_CC;
    let mut out = [0u8; size_of::<u32>()];
    let expected = INPUT.encode();

    to_bytes(out.as_mut(), &INPUT)?;

    assert_eq!(out.as_ref(), expected);
    Ok(())
}

#[test]
fn primitive_u64() -> Result<()> {
    const INPUT: u64 = 0xFFEE_DDCC_BBAA_9988;
    let mut out = [0u8; size_of::<u64>()];
    let expected = INPUT.encode();

    to_bytes(out.as_mut(), &INPUT)?;

    assert_eq!(out.as_mut(), expected);
    Ok(())
}

#[test]
fn primitive_u128() -> Result<()> {
    const INPUT: u128 = 0xFFEE_DDCC_BBAA_9988_7766_5544_3322_1100;
    let mut out = [0u8; size_of::<u128>()];
    let expected = INPUT.encode();

    to_bytes(out.as_mut(), &INPUT)?;

    assert_eq!(out.as_mut(), expected);
    Ok(())
}

#[test]
fn primitive_i16() -> Result<()> {
    const INPUT: i16 = i16::MIN;
    let mut out = [0u8; size_of::<i16>()];
    let expected = INPUT.encode();

    to_bytes(out.as_mut(), &INPUT)?;

    assert_eq!(out.as_mut(), expected);
    Ok(())
}

#[test]
fn primitive_i32() -> Result<()> {
    const INPUT: i32 = i32::MIN;
    let mut out = [0u8; size_of::<i32>()];
    let expected = INPUT.encode();

    to_bytes(out.as_mut(), &INPUT)?;

    assert_eq!(out.as_mut(), expected);
    Ok(())
}

#[test]
fn primitive_i64() -> Result<()> {
    const INPUT: i64 = i64::MIN;
    let mut out = [0u8; size_of::<i64>()];
    let expected = INPUT.encode();

    to_bytes(out.as_mut(), &INPUT)?;

    assert_eq!(out.as_mut(), expected);
    Ok(())
}

#[test]
fn primitive_bool() -> Result<()> {
    const INPUT: bool = true;
    let mut out = [0u8];
    let expected = INPUT.encode();

    to_bytes(out.as_mut(), &INPUT)?;

    assert_eq!(out.as_mut(), expected);
    Ok(())
}

#[test]
fn str() -> Result<()> {
    const INPUT: &str = "ac orci phasellus egestas tellus rutrum tellus pellentesque";
    let mut out = Vec::<u8>::new();
    let expected = INPUT.encode();

    to_bytes(&mut out, &INPUT)?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn bytes() -> Result<()> {
    const INPUT: &[u8] = b"dictumst quisque sagittis purus sit amet volutpat consequat";
    let mut out = Vec::<u8>::new();
    let expected = INPUT.encode();

    to_bytes(&mut out, &INPUT)?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn tuple_simple() -> Result<()> {
    const INPUT: (u8, bool, u64) = (0xD0, false, u64::MAX);
    let mut out = Vec::<u8>::new();
    let expected = INPUT.encode();

    to_bytes(&mut out, &INPUT)?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn enum_simple() -> Result<()> {
    #[derive(Serialize, Encode)]
    enum X {
        _A,
        B,
    }

    const INPUT: X = X::B;
    let mut out = Vec::<u8>::new();
    let expected = INPUT.encode();

    to_bytes(&mut out, &INPUT)?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn tuple_enum_mix() -> Result<()> {
    #[derive(Serialize, Encode)]
    enum X {
        A,
        B,
    }

    let input: (Option<()>, Option<String>, X, X) = (None, Some("hello".into()), X::A, X::B);
    let mut out = Vec::<u8>::new();
    let expected = input.encode();

    to_bytes(&mut out, &input)?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn struct_simple() -> Result<()> {
    #[derive(Serialize, Encode)]
    struct Foo {
        a: Bar,
        b: Option<Baz>,
    }
    #[derive(Serialize, Encode)]
    struct Bar(u8);
    #[derive(Serialize, Encode)]
    struct Baz(String, u16);

    let input = Foo {
        a: Bar(0xFF),
        b: Some(Baz("lol".into(), u16::MAX)),
    };
    let mut out = Vec::<u8>::new();
    let expected = input.encode();

    to_bytes(&mut out, &input)?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn vec_simple() -> Result<()> {
    let input: Vec<String> = vec!["hello".into(), "beautiful".into(), "people".into()];
    let mut out = Vec::<u8>::new();
    let expected = input.encode();

    to_bytes(&mut out, &input)?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn struct_mix() -> Result<()> {
    #[derive(Serialize, Encode)]
    struct Foo<'a> {
        a: Vec<String>,
        b: (Bar<'a>, Bar<'a>, Bar<'a>),
    }
    #[derive(Serialize, Encode)]
    enum Bar<'a> {
        A { thing: &'a str },
        B(Baz),
        C(BTreeMap<String, bool>, i64),
    }
    #[derive(Serialize, Encode)]
    struct Baz;

    let input = Foo {
        a: vec!["hello".into(), "beautiful".into(), "people".into()],
        b: (
            Bar::A { thing: "barbarbar" },
            Bar::B(Baz),
            Bar::C(
                {
                    let mut h = BTreeMap::new();
                    h.insert("key".into(), false);
                    h
                },
                i64::MIN,
            ),
        ),
    };
    let mut out = Vec::<u8>::new();
    let expected = input.encode();

    to_bytes(&mut out, &input)?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn str_as_u128() -> Result<()> {
    const INPUT: &str = "340282366920938463463374607431768211455";
    let mut out = [0u8; size_of::<u128>()];
    let expected = u128::MAX.encode();

    let (id, reg) = register(&0u128);

    to_bytes_with_info(out.as_mut(), &INPUT, Some((&reg, id)))?;

    assert_eq!(out.as_mut(), expected);
    Ok(())
}

#[test]
fn json_simple() -> Result<()> {
    #[derive(Debug, Serialize, Encode, TypeInfo)]
    struct Foo {
        a: Bar,
        b: Option<Baz>,
    }
    #[derive(Debug, Serialize, Encode, TypeInfo)]
    struct Bar(u8);
    #[derive(Debug, Serialize, Encode, TypeInfo)]
    struct Baz(String, i32);

    let input = Foo {
        a: Bar(0xFF),
        b: Some(Baz("lol".into(), i32::MIN)),
    };
    let mut out = Vec::<u8>::new();
    let expected = input.encode();
    let (id, reg) = register(&input);

    let json_input = to_value(&input).unwrap();
    to_bytes_with_info(&mut out, &json_input, Some((&reg, id)))?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn json_mix() -> Result<()> {
    #[derive(Debug, Serialize, Encode, TypeInfo)]
    struct Foo {
        a: Vec<String>,
        b: (Bar, Bar, Bar),
    }
    #[derive(Debug, Serialize, Encode, TypeInfo)]
    enum Bar {
        A { thing: &'static str },
        B(Baz),
        C(BTreeMap<String, bool>, i64),
    }
    #[derive(Debug, Serialize, Encode, TypeInfo)]
    struct Baz;

    let input = Foo {
        a: vec!["hello".into(), "beautiful".into(), "people".into()],
        b: (
            Bar::A { thing: "barbarbar" },
            Bar::B(Baz),
            Bar::C(
                {
                    let mut h = BTreeMap::new();
                    h.insert("key1".into(), false);
                    h.insert("key2".into(), true);
                    h
                },
                i64::MIN,
            ),
        ),
    };
    let mut out = Vec::<u8>::new();
    let expected = input.encode();
    let (id, reg) = register(&input);

    let json_input = to_value(&input).unwrap();
    to_bytes_with_info(&mut out, &json_input, Some((&reg, id)))?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn json_mix2() -> Result<()> {
    #[derive(Debug, Encode, Serialize, TypeInfo)]
    enum Bar {
        This,
        That(i16),
    }
    #[derive(Debug, Encode, Serialize, TypeInfo)]
    struct Baz(String);
    #[derive(Debug, Encode, Serialize, TypeInfo)]
    struct Foo {
        bar: Vec<Bar>,
        baz: Option<Baz>,
        lol: &'static [u8],
    }
    let input = Foo {
        bar: [Bar::That(i16::MAX), Bar::This].into(),
        baz: Some(Baz("lorem ipsum".into())),
        lol: b"\xFFsome stuff\x00",
    };
    let mut out = Vec::<u8>::new();
    let expected = input.encode();
    let (id, reg) = register(&input);

    let json_input = to_value(&input).unwrap();
    to_bytes_with_info(&mut out, &json_input, Some((&reg, id)))?;

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn test_unordered_iter() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    enum Bar {
        _This,
        That(i16),
    }
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        bar: Bar,
        baz: Option<u32>,
        bam: String,
    }
    let foo = Foo {
        bar: Bar::That(i16::MAX),
        baz: Some(123),
        bam: "lorem ipsum".into(),
    };
    let (ty, reg) = register(&foo);

    let input = vec![
        ("bam", crate::JsonValue::String("lol".into())),
        ("baz", 123.into()),
        ("bam", "lorem ipsum".into()),
        ("bar", serde_json::json!({ "That": i16::MAX })),
    ];

    let out = to_vec_from_iter(input, (&reg, ty))?;
    let expected = foo.encode();

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn test_bytes_as_hex_string() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        bar: Vec<u8>,
    }
    let foo = Foo {
        bar: b"\x00\x12\x34\x56".to_vec(),
    };
    let (ty, reg) = register(&foo);

    let hex_string = "0x00123456";

    let input = vec![("bar", crate::JsonValue::String(hex_string.into()))];

    let out = to_vec_from_iter(input, (&reg, ty))?;
    let expected = foo.encode();

    assert_eq!(out, expected);
    Ok(())
}

#[test]
fn test_extrincic_call() -> Result<()> {
    let bytes = include_bytes!("../registry.bin");
    let portable = PortableRegistry::decode(&mut &bytes[..]).expect("hello");
    let registry = crate::compress::compress(&portable);

    let transfer_call = serde_json::json!({
        "transfer_keep_alive": {
            "dest": {
                "Id": hex::decode("12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b").expect("expected valid address")
            },
            "value": 1_000_000_000_000u64
        }
    });

    let call_data =
        to_vec_with_info(&transfer_call, (&registry, 106u32).into()).expect("call data");

    let encooded = hex::encode(call_data);

    assert_eq!(
        "0x04030012840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b070010a5d4e8",
        format!("0x04{}", encooded)
    );

    Ok(())
}

// --- Type coercion tests ---

#[test]
fn json_u64_coerced_to_u8() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        val: u8,
    }
    let foo = Foo { val: 42 };
    let (id, reg) = register(&foo);
    let json = serde_json::to_value(&foo).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, foo.encode());
    Ok(())
}

#[test]
fn json_u64_coerced_to_u16() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        val: u16,
    }
    let foo = Foo { val: 1000 };
    let (id, reg) = register(&foo);
    let json = serde_json::to_value(&foo).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, foo.encode());
    Ok(())
}

#[test]
fn json_u64_coerced_to_u32() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        val: u32,
    }
    let foo = Foo { val: 100_000 };
    let (id, reg) = register(&foo);
    let json = serde_json::to_value(&foo).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, foo.encode());
    Ok(())
}

#[test]
fn json_u64_coerced_to_i32() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        val: i32,
    }
    let foo = Foo { val: -1 };
    let (id, reg) = register(&foo);
    let json = serde_json::to_value(&foo).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, foo.encode());
    Ok(())
}

#[test]
fn json_string_coerced_to_u8() -> Result<()> {
    let (id, reg) = register(&0u8);
    let mut out = Vec::new();
    to_bytes_with_info(&mut out, &"255", Some((&reg, id)))?;
    assert_eq!(out, [255u8]);
    Ok(())
}

#[test]
fn json_string_coerced_to_i64() -> Result<()> {
    let (id, reg) = register(&0i64);
    let mut out = Vec::new();
    to_bytes_with_info(&mut out, &"-9223372036854775808", Some((&reg, id)))?;
    assert_eq!(out, i64::MIN.encode());
    Ok(())
}

#[test]
fn json_compact_u32() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        #[codec(compact)]
        val: u32,
    }
    let foo = Foo { val: 69 };
    let (id, reg) = register(&foo);
    let json = serde_json::to_value(&foo).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, foo.encode());
    Ok(())
}

// --- Option handling ---

#[test]
fn json_option_some_nested() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        val: Option<u32>,
    }
    let foo = Foo { val: Some(42) };
    let (id, reg) = register(&foo);
    let json = serde_json::to_value(&foo).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, foo.encode());
    Ok(())
}

// NOTE: json_option_none is not tested because serde_json serializes
// null via serialize_unit(), not serialize_none(), so the type-info
// path treats it as Some(unit). This is a known JSON→SCALE limitation.

// --- Error paths ---

#[test]
fn invalid_variant_name() {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    enum Bar {
        A,
        #[allow(dead_code)]
        B,
    }
    let (id, reg) = register(&Bar::A);
    let json = serde_json::json!("NonExistent");

    let result = to_vec_with_info(&json, Some((&reg, id)));
    assert!(result.is_err());
}

#[test]
fn invalid_numeric_string() {
    let (id, reg) = register(&0u32);
    let result = to_vec_with_info(&"not_a_number", Some((&reg, id)));
    assert!(result.is_err());
}

#[test]
fn invalid_hex_string() {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        bar: Vec<u8>,
    }
    let foo = Foo { bar: vec![] };
    let (ty, reg) = register(&foo);

    // Missing 0x prefix
    let input = vec![("bar", crate::JsonValue::String("00123456".into()))];
    let result = to_vec_from_iter(input, (&reg, ty));
    assert!(result.is_err());

    // Invalid hex chars
    let input = vec![("bar", crate::JsonValue::String("0xGGHH".into()))];
    let result = to_vec_from_iter(input, (&reg, ty));
    assert!(result.is_err());
}

#[test]
fn from_iter_missing_field() {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        bar: u32,
        baz: String,
    }
    let foo = Foo {
        bar: 1,
        baz: "x".into(),
    };
    let (ty, reg) = register(&foo);

    // Only provide one field
    let input = vec![("bar", crate::JsonValue::from(1))];
    let result = to_vec_from_iter(input, (&reg, ty));
    assert!(result.is_err());
}

// --- Nested struct variants ---

#[test]
fn json_struct_variant() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    enum Msg {
        #[allow(dead_code)]
        Ping,
        Data {
            id: u32,
            payload: String,
        },
    }
    let input = Msg::Data {
        id: 42,
        payload: "hello".into(),
    };
    let (id, reg) = register(&input);
    let json = serde_json::to_value(&input).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, input.encode());
    Ok(())
}

#[test]
fn json_tuple_variant() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    enum Msg {
        #[allow(dead_code)]
        Ping,
        Pair(u32, u32),
    }
    let input = Msg::Pair(1, 2);
    let (id, reg) = register(&input);
    let json = serde_json::to_value(&input).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, input.encode());
    Ok(())
}

#[test]
fn json_array_type() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        data: [u8; 4],
    }
    let foo = Foo { data: [1, 2, 3, 4] };
    let (id, reg) = register(&foo);
    let json = serde_json::to_value(&foo).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, foo.encode());
    Ok(())
}

#[test]
fn json_btreemap() -> Result<()> {
    // NOTE: Map values aren't type-coerced (TypedSerializer::Empty for Map),
    // so we use bool values which don't need coercion (same size in JSON and SCALE)
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    struct Foo {
        map: BTreeMap<String, bool>,
    }
    let foo = Foo {
        map: {
            let mut m = BTreeMap::new();
            m.insert("a".into(), true);
            m.insert("b".into(), false);
            m
        },
    };
    let (id, reg) = register(&foo);
    let json = serde_json::to_value(&foo).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, foo.encode());
    Ok(())
}

#[test]
fn json_nested_option_in_enum() -> Result<()> {
    #[derive(Debug, Encode, TypeInfo, Serialize)]
    enum Outer {
        #[allow(dead_code)]
        None,
        Some(Option<u32>),
    }
    let input = Outer::Some(Some(99));
    let (id, reg) = register(&input);
    let json = serde_json::to_value(&input).unwrap();

    let out = to_vec_with_info(&json, Some((&reg, id)))?;
    assert_eq!(out, input.encode());
    Ok(())
}
