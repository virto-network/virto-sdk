use super::register;
use crate::textfmt::{from_text, to_text};
use crate::Value;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use codec::Encode;

fn roundtrip<T: scale_info::TypeInfo + Encode + 'static>(val: &T, expected_text: &str) {
    let encoded = val.encode();
    let (ty_id, reg) = register(val);
    let value = Value::new(&encoded, ty_id, &reg);

    let text = to_text(&value).expect("format failed");
    assert_eq!(text, expected_text, "format mismatch");

    let decoded = from_text(&text, &reg, ty_id).expect("parse failed");
    assert_eq!(decoded, encoded, "roundtrip mismatch for '{text}'");
}

#[test]
fn primitives() {
    roundtrip(&42u8, "42");
    roundtrip(&1000u16, "1000");
    roundtrip(&123456u32, "123456");
    roundtrip(&999u64, "999");
    roundtrip(&0u128, "0");
    roundtrip(&true, "true");
    roundtrip(&false, "false");
    roundtrip(&-1i8, "-1");
    roundtrip(&-1000i16, "-1000");
    roundtrip(&-42i32, "-42");
}

#[test]
fn strings() {
    roundtrip(&String::from("hello"), "'hello'");
    roundtrip(&String::from(""), "''");
    roundtrip(&String::from("foo bar"), "'foo bar'");
    roundtrip(&String::from("it's"), "'it''s'");
    roundtrip(&String::from("'"), "''''");
}

#[test]
fn integer_overflow_rejected() {
    use crate::registry::{Registry, TypeDefOwned};

    let reg = Registry::new(alloc::vec![TypeDefOwned::U8]);
    assert!(from_text("256", &reg, 0).is_err());
    assert!(from_text("0", &reg, 0).is_ok());
    assert!(from_text("255", &reg, 0).is_ok());

    let reg = Registry::new(alloc::vec![TypeDefOwned::U16]);
    assert!(from_text("65536", &reg, 0).is_err());
    assert!(from_text("65535", &reg, 0).is_ok());

    let reg = Registry::new(alloc::vec![TypeDefOwned::I8]);
    assert!(from_text("-129", &reg, 0).is_err());
    assert!(from_text("128", &reg, 0).is_err());
    assert!(from_text("-128", &reg, 0).is_ok());
    assert!(from_text("127", &reg, 0).is_ok());
}

// ── Security tests ─────────────────────────────────────────────────────

#[test]
fn recursive_type_depth_limit() {
    use crate::registry::{Registry, TypeDefOwned};

    // StructNewType(0) at index 0 creates a self-referencing cycle
    let reg = Registry::new(alloc::vec![TypeDefOwned::StructNewType(0)]);
    let err = from_text("42", &reg, 0);
    assert!(err.is_err());
    let msg = alloc::format!("{}", err.unwrap_err());
    assert!(msg.contains("depth"), "expected depth error, got: {msg}");
}

#[test]
fn trailing_input_truncated_in_error() {
    use crate::registry::{Registry, TypeDefOwned};

    let reg = Registry::new(alloc::vec![TypeDefOwned::U8]);
    let long_trail = alloc::format!("1{}", "x".repeat(200));
    let err = from_text(&long_trail, &reg, 0).unwrap_err();
    let msg = alloc::format!("{err}");
    // The error message should not echo the full 200 chars of trailing input
    assert!(msg.len() < 120, "error too long, may echo raw input: {msg}");
    assert!(msg.contains("..."), "should be truncated: {msg}");
}

#[test]
fn bool_rejects_prefix_match() {
    use crate::registry::{Registry, TypeDefOwned};

    let reg = Registry::new(alloc::vec![TypeDefOwned::Bool]);
    // "truer" should not parse as true — the 'r' isn't a valid delimiter
    assert!(from_text("truer", &reg, 0).is_err());
    assert!(from_text("falsehood", &reg, 0).is_err());
    // Correct cases still work
    assert!(from_text("true", &reg, 0).is_ok());
    assert!(from_text("false", &reg, 0).is_ok());
}

#[test]
fn unterminated_string_rejected() {
    use crate::registry::{Registry, TypeDefOwned};

    let reg = Registry::new(alloc::vec![TypeDefOwned::Str]);
    assert!(from_text("'hello", &reg, 0).is_err());
    assert!(from_text("'", &reg, 0).is_err());
}

#[test]
fn odd_hex_rejected() {
    use crate::registry::{Registry, TypeDefOwned};

    let reg = Registry::new(alloc::vec![TypeDefOwned::Bytes]);
    assert!(from_text("0xabc", &reg, 0).is_err()); // 3 hex digits
    assert!(from_text("0xab", &reg, 0).is_ok());
}

#[test]
fn invalid_variant_rejected() {
    #[derive(Encode, scale_info::TypeInfo)]
    enum AB {
        A,
        #[allow(dead_code)]
        B,
    }
    let (ty_id, reg) = register(&AB::A);
    assert!(from_text("AB::C", &reg, ty_id).is_err());
    assert!(from_text("Wrong::A", &reg, ty_id).is_err());
    assert!(from_text("AB::A", &reg, ty_id).is_ok());
}

#[test]
fn wrong_field_name_rejected() {
    #[derive(Encode, scale_info::TypeInfo)]
    struct Foo {
        bar: u32,
    }
    let (ty_id, reg) = register(&Foo { bar: 0 });
    assert!(from_text("(baz:1)", &reg, ty_id).is_err());
    assert!(from_text("(bar:1)", &reg, ty_id).is_ok());
}

#[test]
fn empty_input_for_non_unit_rejected() {
    use crate::registry::{Registry, TypeDefOwned};

    let reg = Registry::new(alloc::vec![TypeDefOwned::U32]);
    assert!(from_text("", &reg, 0).is_err());
}

#[test]
fn missing_sequence_close_rejected() {
    use crate::registry::{Registry, TypeDefOwned};

    // Sequence(U8) at index 0, U8 at index 1
    let reg = Registry::new(alloc::vec![TypeDefOwned::Sequence(1), TypeDefOwned::U8]);
    assert!(from_text("..1;2;3", &reg, 0).is_err()); // no closing .
    assert!(from_text("..1;2;3.", &reg, 0).is_ok());
}

#[test]
fn string_with_special_chars_roundtrip() {
    // Strings with characters that have meaning in the format syntax
    roundtrip(&String::from("a;b"), "'a;b'");
    roundtrip(&String::from("a:b"), "'a:b'");
    roundtrip(&String::from("a(b)"), "'a(b)'");
    roundtrip(&String::from("..x."), "'..x.'");
    roundtrip(&String::from("Foo::Bar"), "'Foo::Bar'");
}

#[test]
fn display_impl() {
    let encoded = 42u32.encode();
    let (ty_id, reg) = register(&42u32);
    let value = Value::new(&encoded, ty_id, &reg);
    assert_eq!(alloc::format!("{value}"), "42");
}

#[test]
fn tuples() {
    roundtrip(&(1u8, 2u16), "(1;2)");
    roundtrip(&(true, 42u32, String::from("x")), "(true;42;'x')");
}

#[test]
fn option_variants() {
    roundtrip(&Some(42u32), "Option::Some(42)");
    roundtrip(&Option::<u32>::None, "Option::None");
}

#[test]
fn sequences() {
    // Vec<u8> compresses to TypeDef::Bytes, so use Vec<u16> for sequences
    roundtrip(&vec![1u16, 2, 3], "..1;2;3.");
    roundtrip(&Vec::<u32>::new(), "...");
    roundtrip(&vec![vec![1u16, 2], vec![3u16]], "....1;2.;..3..");
}

#[test]
fn bytes_hex() {
    roundtrip(&vec![0x01u8, 0x02, 0xAB], "0x0102ab");
    roundtrip(&Vec::<u8>::new(), "0x");
}

#[test]
fn arrays() {
    // Byte arrays use compact 0x hex notation
    roundtrip(&[10u8, 20, 30], "0x0a141e");
    // Non-byte arrays still use the ..elem;elem. notation
    roundtrip(&[100u32, 200, 300], "..100;200;300.");
}

#[test]
fn named_struct() {
    #[derive(Encode, scale_info::TypeInfo)]
    struct Point {
        x: u32,
        y: u32,
    }
    roundtrip(&Point { x: 1, y: 2 }, "(x:1;y:2)");
}

#[test]
fn nested_struct() {
    #[derive(Encode, scale_info::TypeInfo)]
    struct Inner {
        a: u8,
        b: bool,
    }
    #[derive(Encode, scale_info::TypeInfo)]
    struct Outer {
        name: String,
        inner: Inner,
    }
    roundtrip(
        &Outer {
            name: String::from("test"),
            inner: Inner { a: 1, b: true },
        },
        "(name:'test';inner:(a:1;b:true))",
    );
}

#[test]
fn enum_variants() {
    #[derive(Encode, scale_info::TypeInfo)]
    enum Color {
        Red,
        Green,
        Blue,
    }
    roundtrip(&Color::Red, "Color::Red");
    roundtrip(&Color::Green, "Color::Green");
    roundtrip(&Color::Blue, "Color::Blue");
}

#[test]
fn enum_with_data() {
    #[derive(Encode, scale_info::TypeInfo)]
    enum Action {
        Transfer { from: u32, to: u32, amount: u64 },
        Approve(u32),
    }
    roundtrip(
        &Action::Transfer {
            from: 1,
            to: 2,
            amount: 1000,
        },
        "Action::Transfer(from:1;to:2;amount:1000)",
    );
    roundtrip(&Action::Approve(42), "Action::Approve(42)");
}

#[test]
fn struct_with_sequence() {
    #[derive(Encode, scale_info::TypeInfo)]
    struct Staker {
        era: u32,
        validators: Vec<u32>,
        active: bool,
    }
    roundtrip(
        &Staker {
            era: 42,
            validators: vec![1, 2, 3],
            active: true,
        },
        "(era:42;validators:..1;2;3.;active:true)",
    );
}

#[test]
fn complex_nested() {
    #[derive(Encode, scale_info::TypeInfo)]
    struct Nom {
        who: u32,
        value: u64,
    }
    #[derive(Encode, scale_info::TypeInfo)]
    struct Exposure {
        total: u64,
        own: u64,
        others: Vec<Nom>,
    }
    roundtrip(
        &Exposure {
            total: 1000,
            own: 500,
            others: vec![Nom { who: 1, value: 300 }, Nom { who: 2, value: 200 }],
        },
        "(total:1000;own:500;others:..(who:1;value:300);(who:2;value:200).)",
    );
}

#[test]
fn map_type() {
    let mut m = BTreeMap::new();
    m.insert(1u32, 100u64);
    m.insert(2, 200);
    roundtrip(&m, "..(1;100);(2;200).");
}

#[test]
fn parse_then_format() {
    // Verify that parsing produces valid SCALE bytes that can be re-formatted
    #[derive(Encode, scale_info::TypeInfo)]
    struct Pair {
        a: u32,
        b: bool,
    }
    let (ty_id, reg) = register(&Pair { a: 0, b: false });

    let input = "(a:99;b:true)";
    let bytes = from_text(input, &reg, ty_id).unwrap();
    let value = Value::new(&bytes, ty_id, &reg);
    let output = to_text(&value).unwrap();
    assert_eq!(output, input);
}
