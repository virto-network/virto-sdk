#![cfg_attr(not(feature = "std"), no_std)]
///! # Scales
///!
///! Dynamic SCALE Serialization using `scale-info` type information.
///!

#[cfg(feature = "serializer")]
mod serializer;
mod value;

#[cfg(feature = "serializer")]
pub use serializer::{to_writer, Serializer};
pub use value::Value;

use scale_info::{Field, MetaType, Type, Variant};

/// A convenient representation of the scale-info types to a format
/// that matches serde model more closely
#[rustfmt::skip]
#[derive(Debug)]
enum SerdeType<'a> {
    Bool,
    U8, U16, U32, U64, U128,
    I8, I16, I32, I64, I128,
    Bytes,
    Char,
    Str,
    Sequence(Type),
    Map(Type, Type),
    Tuple(TupleOrArray<'a>),
    Struct(&'a [Field]), StructUnit, StructNewType(Type), StructTuple(&'a [Field]),
    Variant(&'a str, &'a [Variant]),
}

impl<'a> From<&'a Type> for SerdeType<'a> {
    fn from(ty: &'a Type) -> Self {
        use scale_info::{TypeDef, TypeDef::*, TypeDefComposite, TypeDefPrimitive};
        let name = ty.path().segments().last().copied().unwrap_or("");

        #[inline]
        fn is_map(ty: &Type) -> bool {
            ty.path().segments() == ["BTreeMap"]
        }
        fn map_types(ty: &TypeDefComposite) -> (Type, Type) {
            let field = ty.fields().first().expect("map");
            // Type information of BTreeMap is weirdly packed
            if let TypeDef::Sequence(s) = field.ty().type_info().type_def() {
                if let TypeDef::Tuple(t) = s.type_param().type_info().type_def() {
                    assert_eq!(t.fields().len(), 2);
                    let key_ty = t.fields().first().expect("key").type_info();
                    let val_ty = t.fields().last().expect("val").type_info();
                    return (key_ty, val_ty);
                }
            }
            unreachable!()
        }

        match ty.type_def() {
            Composite(c) => {
                let fields = c.fields();
                if fields.is_empty() {
                    Self::StructUnit
                } else if is_map(&ty) {
                    let (k, v) = map_types(c);
                    Self::Map(k, v)
                } else if fields.len() == 1 {
                    Self::StructNewType(fields.first().unwrap().ty().type_info())
                } else if is_tuple(fields) {
                    Self::StructTuple(fields)
                } else {
                    Self::Struct(fields)
                }
            }
            Variant(v) => Self::Variant(name, v.variants()),
            Sequence(s) => Self::Sequence(s.type_param().type_info()),
            Array(a) => Self::Tuple(TupleOrArray::Array(a.type_param(), a.len())),
            Tuple(t) => Self::Tuple(TupleOrArray::Tuple(t.fields())),
            Primitive(p) => match p {
                TypeDefPrimitive::U8 => Self::U8,
                TypeDefPrimitive::U16 => Self::U16,
                TypeDefPrimitive::U32 => Self::U32,
                TypeDefPrimitive::U64 => Self::U64,
                TypeDefPrimitive::U128 => Self::U128,
                TypeDefPrimitive::I8 => Self::I8,
                TypeDefPrimitive::I16 => Self::I16,
                TypeDefPrimitive::I32 => Self::I32,
                TypeDefPrimitive::I64 => Self::I64,
                TypeDefPrimitive::I128 => Self::I128,
                TypeDefPrimitive::Bool => Self::Bool,
                TypeDefPrimitive::Str => Self::Str,
                TypeDefPrimitive::Char => Self::Char,
                TypeDefPrimitive::U256 => unimplemented!(),
                TypeDefPrimitive::I256 => unimplemented!(),
            },
            Compact(_c) => todo!(),
            BitSequence(_b) => todo!(),
        }
    }
}

impl<'a> SerdeType<'a> {
    fn pick_variant(self, index: u8) -> EnumVariant<'a> {
        match self {
            SerdeType::Variant(name, variants) => {
                let variant = variants
                    .iter()
                    .find(|v| v.index() == index)
                    .expect("variant");
                let fields = variant.fields();
                if fields.is_empty() {
                    if name == "Option" && variant.name() == &"None" {
                        EnumVariant::OptionNone.into()
                    } else {
                        EnumVariant::Unit(variant).into()
                    }
                } else if is_tuple(fields) {
                    if fields.len() == 1 {
                        let ty = fields.first().unwrap().ty().type_info();
                        return if name == "Option" && variant.name() == &"Some" {
                            EnumVariant::OptionSome(ty).into()
                        } else {
                            EnumVariant::NewType(variant).into()
                        };
                    } else {
                        EnumVariant::Tuple(variant).into()
                    }
                } else {
                    EnumVariant::Struct(variant).into()
                }
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
enum EnumVariant<'a> {
    OptionNone,
    OptionSome(Type),
    Unit(&'a Variant),
    NewType(&'a Variant),
    Tuple(&'a Variant),
    Struct(&'a Variant),
}

#[derive(Debug)]
enum TupleOrArray<'a> {
    Array(&'a MetaType, u32),
    Tuple(&'a [MetaType]),
}
impl<'a> TupleOrArray<'a> {
    fn len(&self) -> usize {
        match self {
            Self::Array(_, len) => *len as usize,
            Self::Tuple(fields) => fields.len(),
        }
    }

    fn type_info(&self, i: usize) -> Type {
        match self {
            Self::Array(ty, _) => ty.type_info(),
            Self::Tuple(fields) => fields[i].type_info(),
        }
    }
}

#[inline]
fn is_tuple(fields: &[Field]) -> bool {
    fields.first().and_then(Field::name).is_none()
}
