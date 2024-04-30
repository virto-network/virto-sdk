#![cfg_attr(not(feature = "std"), no_std)]
//!
//! # Scales
//!
//! Dynamic SCALE Serialization using `scale-info` type information.

#[macro_use]
extern crate alloc;

#[cfg(feature = "experimental-serializer")]
mod serializer;
mod value;

pub use bytes::Bytes;
#[cfg(feature = "json")]
pub use serde_json::Value as JsonValue;
#[cfg(feature = "experimental-serializer")]
pub use serializer::{to_bytes, to_bytes_with_info, to_vec, to_vec_with_info, Serializer};
#[cfg(feature = "json")]
pub use serializer::{to_bytes_from_iter, to_vec_from_iter};
pub use value::Value;

use prelude::*;
use scale_info::{form::PortableForm as Portable, PortableRegistry};

mod prelude {
    pub use core::ops::Deref;
}

macro_rules! is_tuple {
    ($it:ident) => {
        $it.fields.first().and_then(|f| f.name.as_ref()).is_none()
    };
}

type Field = scale_info::Field<Portable>;
type Type = scale_info::Type<Portable>;
type Variant = scale_info::Variant<Portable>;
type TypeId = u32;

type Map<'a, F> = core::iter::Map<core::slice::Iter<'a, Field>, F>;
type NameNTypeFromField<'a> = Map<'a, fn(&Field) -> (&str, TypeId)>;
type TypeFromField<'a> = Map<'a, fn(&Field) -> TypeId>;
type Variants<'a> = core::slice::Iter<'a, Variant>;

fn field_type(f: &Field) -> TypeId {
    f.ty.id
}
fn field_name_n_type(f: &Field) -> (&str, TypeId) {
    (f.name.expect("field name").as_ref(), f.ty.id)
}

/// A convenient representation of the scale-info types to a format
/// that matches serde model more closely
#[rustfmt::skip]
#[derive(Debug, Clone )]
// #[derive(Debug, Clone, serde::Serialize)]
// #[cfg_attr(feature = "codec", derive(codec::Encode))]
pub enum SpecificType<'r> {
    Bool,
    U8, U16, U32, U64, U128,
    I8, I16, I32, I64, I128,
    Char,
    Str,
    Bytes(TypeId),
    Sequence(TypeId),
    Map(TypeId, TypeId),
    Tuple(TupleOrArray<'r>),
    Struct(NameNTypeFromField<'r>), 
    StructNewType(TypeId), StructTuple(TypeFromField<'r>), StructUnit,
    Variant(&'r str, Variants<'r>),
    Compact(TypeId),
}

impl<'r> From<(&Type, &PortableRegistry)> for SpecificType<'r> {
    fn from((ty, registry): (&Type, &PortableRegistry)) -> Self {
        use scale_info::{TypeDef, TypeDefComposite, TypeDefPrimitive};
        type Def = TypeDef<Portable>;

        macro_rules! resolve {
            ($ty:expr) => {
                registry.resolve($ty.id).unwrap()
            };
        }
        let is_map = |ty: &Type| -> bool { ty.path.segments == ["BTreeMap"] };
        let map_types = |ty: &TypeDefComposite<Portable>| -> (TypeId, TypeId) {
            let field = ty.fields.first().expect("map");
            // Type information of BTreeMap is weirdly packed
            if let Def::Sequence(s) = &resolve!(field.ty).type_def {
                if let Def::Tuple(t) = &resolve!(s.type_param).type_def {
                    assert_eq!(t.fields.len(), 2);
                    let key_ty = t.fields.first().expect("key").id;
                    let val_ty = t.fields.last().expect("val").id;
                    return (key_ty, val_ty);
                }
            }
            unreachable!()
        };

        let name = ty
            .path
            .segments
            .last()
            .cloned()
            .unwrap_or_else(|| "".into());

        match ty.type_def {
            Def::Composite(ref c) => {
                let fields = &c.fields;
                if fields.is_empty() {
                    Self::StructUnit
                } else if is_map(ty) {
                    let (k, v) = map_types(c);
                    Self::Map(k, v)
                } else if fields.len() == 1 && fields.first().unwrap().name.is_none() {
                    Self::StructNewType(fields.first().unwrap().ty.id)
                } else if is_tuple!(c) {
                    Self::StructTuple(fields.iter().map(field_type))
                } else {
                    Self::Struct(fields.iter().map(field_name_n_type))
                }
            }
            Def::Variant(ref v) => Self::Variant(&name, v.variants.iter()),
            Def::Sequence(ref s) => {
                let ty = s.type_param;
                if matches!(resolve!(ty).type_def, Def::Primitive(TypeDefPrimitive::U8)) {
                    Self::Bytes(ty.id)
                } else {
                    Self::Sequence(ty.id)
                }
            }
            Def::Array(ref a) => Self::Tuple(TupleOrArray::Array(a.type_param.id, a.len)),
            Def::Tuple(ref t) => {
                Self::Tuple(TupleOrArray::Tuple(t.fields.iter().map(|f| resolve!(f))))
            }
            Def::Primitive(ref p) => match p {
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
            Def::Compact(ref c) => Self::Compact(c.type_param.id),
            Def::BitSequence(ref _b) => todo!(),
        }
    }
}

// Utilities for enum variants
impl<'r> SpecificType<'r> {
    fn pick_variant(self, index: u8) -> Option<VariantKind<'r>> {
        let (ty_name, variant) = self.find_variant(|v| v.index == index)?;
        let fields = &variant.fields;
        let vname = variant.name.as_ref();

        Some(if fields.is_empty() {
            if ty_name == "Option" && vname == "None" {
                VariantKind::OptionNone
            } else {
                VariantKind::Unit(vname)
            }
        } else if is_tuple!(variant) {
            if fields.len() == 1 {
                let ty = fields.first().map(field_type).unwrap();
                if ty_name == "Option" && variant.name == "Some" {
                    VariantKind::OptionSome(ty)
                } else {
                    VariantKind::NewType(vname, ty)
                }
            } else {
                let fields = fields.iter().map(field_type);
                VariantKind::Tuple(vname, fields)
            }
        } else {
            let fields = fields.iter().map(field_name_n_type);
            VariantKind::Struct(vname, fields)
        })
        // match self {
        //     SpecificType::Variant(name, variant, Some(_)) => {
        //         Self::Variant(*name, variant, Some(index))
        //     }
        //     SpecificType::Variant(name, variants, None) => {
        //         let v = variants.iter().find(|v| v.index == index).unwrap();
        //         Self::Variant(*name, vec![v.clone()], Some(index))
        //     }
        //     _ => panic!("Only for enum variants"),
        // }
    }

    fn find_variant(&self, predicate: impl FnMut(&&Variant) -> bool) -> Option<(&str, &Variant)> {
        match self {
            SpecificType::Variant(name, mut variants) => {
                let v = variants.find(predicate)?;
                Some((name, v))
            }
            _ => panic!("Only for enum variants"),
        }
    }

    // #[cfg(feature = "experimental-serializer")]
    // fn pick_mut<F, A, B>(&mut self, selection: A, get_field: F) -> Option<&Self>
    // where
    //     F: Fn(&Variant) -> B,
    //     A: AsRef<[u8]> + PartialEq + core::fmt::Debug,
    //     B: AsRef<[u8]> + PartialEq + core::fmt::Debug,
    // {
    //     match self {
    //         SpecificType::Variant(_, _, Some(_)) => Some(self),
    //         SpecificType::Variant(_, ref mut variants, idx @ None) => {
    //             let (vf, _) = variants
    //                 .iter()
    //                 .map(|v| (v.index, get_field(v)))
    //                 .find(|(_, f)| f.as_ref() == selection.as_ref())?;

    //             variants.retain(|v| v.index == vf);
    //             *idx = Some(vf);

    //             Some(self)
    //         }
    //         _ => panic!("Only for enum variants"),
    //     }
    // }
}

#[derive(Debug)]
enum VariantKind<'a> {
    OptionNone,
    OptionSome(TypeId),
    Unit(&'a str),
    NewType(&'a str, TypeId),
    Tuple(&'a str, TypeFromField<'a>),
    Struct(&'a str, NameNTypeFromField<'a>),
}

// impl<'a> From<&'a SpecificType> for VariantKind<'a> {
//     fn from(ty: &'a SpecificType) -> Self {

//     }
// }

#[derive(Debug, Clone)]
// #[derive(Debug, Clone, serde::Serialize)]
// #[cfg_attr(feature = "codec", derive(codec::Encode))]
pub enum TupleOrArray<'a> {
    Array(TypeId, u32),
    Tuple(TypeFromField<'a>),
}
impl TupleOrArray<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Array(_, len) => *len as usize,
            Self::Tuple(fields) => fields.count(),
        }
    }

    fn type_id(&self, i: usize) -> TypeId {
        match self {
            Self::Array(ty, _) => *ty,
            Self::Tuple(fields) => fields.nth(i).expect("tuple field"),
        }
    }
}

// adapted from https://github.com/paritytech/parity-scale-codec/blob/master/src/compact.rs#L336
#[allow(clippy::all)]
pub fn write_compact(n: u128, mut dest: impl bytes::BufMut) {
    match n {
        0..=0b0011_1111 => dest.put_u8((n as u8) << 2),
        0..=0b0011_1111_1111_1111 => dest.put_u16_le(((n as u16) << 2) | 0b01),
        0..=0b0011_1111_1111_1111_1111_1111_1111_1111 => dest.put_u32_le(((n as u32) << 2) | 0b10),
        _ => {
            let bytes_needed = 8 - n.leading_zeros() / 8;
            assert!(
                bytes_needed >= 4,
                "Previous match arm matches anyting less than 2^30; qed"
            );
            dest.put_u8(0b11 + ((bytes_needed - 4) << 2) as u8);
            let mut v = n;
            for _ in 0..bytes_needed {
                dest.put_u8(v as u8);
                v >>= 8;
            }
            assert_eq!(
                v, 0,
                "shifted sufficient bits right to lead only leading zeros; qed"
            )
        }
    }
}
