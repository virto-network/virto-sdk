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

#[rustfmt::skip]
/// A convenient representation of the types that matches serde model more closely
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
    OptionNone, OptionSome(Type),
    VariantUnit(&'a Variant), VariantNewType(&'a Variant), 
    VariantTuple(&'a Variant), VariantStruct(&'a Variant),
    Struct(&'a [Field]), StructUnit, StructNewType, StructTuple(&'a [Field]),
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

impl<'a> SerdeType<'a> {
    fn from(ty: &'a Type, maybe_variant_index: u8) -> Self {
        use scale_info::{TypeDef, TypeDef::*, TypeDefComposite, TypeDefPrimitive};
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
        #[inline]
        fn is_tuple(fields: &[Field]) -> bool {
            fields.first().and_then(Field::name).is_none()
        }

        let name = ty.path().segments().last().copied().unwrap_or("");

        match ty.type_def() {
            Composite(c) => {
                let fields = c.fields();
                if fields.is_empty() {
                    Self::StructUnit
                } else if is_map(ty) {
                    let (k, v) = map_types(c);
                    Self::Map(k, v)
                } else if is_tuple(fields) {
                    Self::StructTuple(fields)
                } else {
                    Self::Struct(fields)
                }
            }
            Variant(enu) => {
                let variant = enu
                    .variants()
                    .iter()
                    .find(|v| v.index() == maybe_variant_index)
                    .expect("variant");
                let fields = variant.fields();
                if fields.is_empty() {
                    if name == "Option" && variant.name() == &"None" {
                        Self::OptionNone
                    } else {
                        Self::VariantUnit(variant)
                    }
                } else if is_tuple(fields) {
                    if fields.len() == 1 {
                        let ty = fields.first().unwrap().ty().type_info();
                        return if name == "Option" && variant.name() == &"Some" {
                            Self::OptionSome(ty)
                        } else {
                            Self::VariantNewType(variant)
                        };
                    } else {
                        Self::VariantTuple(variant)
                    }
                } else {
                    Self::VariantStruct(variant)
                }
            }
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
