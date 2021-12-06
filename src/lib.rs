#![cfg_attr(not(feature = "std"), no_std)]
///!
///! # Scales
///!
///! Dynamic SCALE Serialization using `scale-info` type information.
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
    pub use alloc::{
        collections::BTreeMap,
        string::{String, ToString},
        vec::Vec,
    };
    pub use core::ops::Deref;
}

type Type = scale_info::Type<Portable>;
type Field = scale_info::Field<Portable>;
type Variant = scale_info::Variant<Portable>;
type TypeId = u32;

macro_rules! is_tuple {
    ($it:ident) => {
        $it.fields().first().and_then(Field::name).is_none()
    };
}

/// A convenient representation of the scale-info types to a format
/// that matches serde model more closely
#[rustfmt::skip]
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "codec", derive(codec::Encode))]
pub enum SpecificType {
    Bool,
    U8, U16, U32, U64, U128,
    I8, I16, I32, I64, I128,
    Char,
    Str,
    Bytes(TypeId),
    Sequence(TypeId),
    Map(TypeId, TypeId),
    Tuple(TupleOrArray),
    Struct(Vec<(String, TypeId)>), StructUnit, StructNewType(TypeId), StructTuple(Vec<TypeId>),
    Variant(String, Vec<Variant>, Option<u8>),
}

impl From<(&Type, &PortableRegistry)> for SpecificType {
    fn from((ty, registry): (&Type, &PortableRegistry)) -> Self {
        use scale_info::{TypeDef, TypeDefComposite, TypeDefPrimitive};
        type Def = TypeDef<Portable>;

        macro_rules! resolve {
            ($ty:expr) => {
                registry.resolve($ty.id()).unwrap()
            };
        }
        let is_map = |ty: &Type| -> bool { ty.path().segments() == ["BTreeMap"] };
        let map_types = |ty: &TypeDefComposite<Portable>| -> (TypeId, TypeId) {
            let field = ty.fields().first().expect("map");
            // Type information of BTreeMap is weirdly packed
            if let Def::Sequence(s) = resolve!(field.ty()).type_def() {
                if let Def::Tuple(t) = resolve!(s.type_param()).type_def() {
                    assert_eq!(t.fields().len(), 2);
                    let key_ty = t.fields().first().expect("key").id();
                    let val_ty = t.fields().last().expect("val").id();
                    return (key_ty, val_ty);
                }
            }
            unreachable!()
        };

        let name = ty
            .path()
            .segments()
            .last()
            .cloned()
            .unwrap_or_else(|| "".into());

        match ty.type_def() {
            Def::Composite(c) => {
                let fields = c.fields();
                if fields.is_empty() {
                    Self::StructUnit
                } else if is_map(ty) {
                    let (k, v) = map_types(c);
                    Self::Map(k, v)
                } else if fields.len() == 1 && fields.first().unwrap().name().is_none() {
                    Self::StructNewType(fields.first().unwrap().ty().id())
                } else if is_tuple!(c) {
                    Self::StructTuple(fields.iter().map(|f| f.ty().id()).collect())
                } else {
                    Self::Struct(
                        fields
                            .iter()
                            .map(|f| (f.name().unwrap().deref().into(), f.ty().id()))
                            .collect(),
                    )
                }
            }
            Def::Variant(v) => Self::Variant(name.into(), v.variants().into(), None),
            Def::Sequence(s) => {
                let ty = s.type_param();
                if matches!(
                    resolve!(ty).type_def(),
                    Def::Primitive(TypeDefPrimitive::U8)
                ) {
                    Self::Bytes(ty.id())
                } else {
                    Self::Sequence(ty.id())
                }
            }
            Def::Array(a) => Self::Tuple(TupleOrArray::Array(a.type_param().id(), a.len())),
            Def::Tuple(t) => Self::Tuple(TupleOrArray::Tuple(
                t.fields().iter().map(|ty| ty.id()).collect(),
            )),
            Def::Primitive(p) => match p {
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
            Def::Compact(_c) => todo!(),
            Def::BitSequence(_b) => todo!(),
        }
    }
}

// Utilities for enum variants
impl SpecificType {
    fn pick(&self, index: u8) -> Self {
        match self {
            SpecificType::Variant(name, variant, Some(_)) => {
                Self::Variant(name.to_string(), variant.to_vec(), Some(index))
            }
            SpecificType::Variant(name, variants, None) => {
                let v = variants.iter().find(|v| v.index() == index).unwrap();
                Self::Variant(name.clone(), vec![v.clone()], Some(index))
            }
            _ => panic!("Only for enum variants"),
        }
    }

    #[cfg(feature = "experimental-serializer")]
    fn pick_mut<F, A, B>(&mut self, selection: A, get_field: F) -> Option<&Self>
    where
        F: Fn(&Variant) -> B,
        A: AsRef<[u8]> + PartialEq + core::fmt::Debug,
        B: AsRef<[u8]> + PartialEq + core::fmt::Debug,
    {
        match self {
            SpecificType::Variant(_, _, Some(_)) => Some(self),
            SpecificType::Variant(_, ref mut variants, idx @ None) => {
                let i = variants
                    .iter()
                    .map(|v| get_field(v))
                    .position(|f| f.as_ref() == selection.as_ref())? as u8;
                variants.retain(|v| v.index() == i);
                *idx = Some(i);
                Some(self)
            }
            _ => panic!("Only for enum variants"),
        }
    }

    #[cfg(feature = "experimental-serializer")]
    fn variant_id(&self) -> u8 {
        match self {
            SpecificType::Variant(_, _, Some(id)) => *id,
            _ => panic!("Only for enum variants"),
        }
    }
}

#[derive(Debug)]
enum EnumVariant<'a> {
    OptionNone,
    OptionSome(TypeId),
    Unit(u8, &'a str),
    NewType(u8, &'a str, TypeId),
    Tuple(u8, &'a str, Vec<TypeId>),
    Struct(u8, &'a str, Vec<(&'a str, TypeId)>),
}

impl<'a> From<&'a SpecificType> for EnumVariant<'a> {
    fn from(ty: &'a SpecificType) -> Self {
        match ty {
            SpecificType::Variant(name, variants, Some(idx)) => {
                let variant = variants.first().expect("single variant");
                let fields = variant.fields();
                let vname = variant.name().as_ref();

                if fields.is_empty() {
                    if name == "Option" && vname == "None" {
                        Self::OptionNone
                    } else {
                        Self::Unit(*idx, &vname)
                    }
                } else if is_tuple!(variant) {
                    if fields.len() == 1 {
                        let ty = fields.first().map(|f| f.ty().id()).unwrap();
                        return if name == "Option" && variant.name() == &"Some" {
                            Self::OptionSome(ty)
                        } else {
                            Self::NewType(*idx, &vname, ty)
                        };
                    } else {
                        let fields = fields.iter().map(|f| f.ty().id()).collect();
                        Self::Tuple(*idx, &vname, fields)
                    }
                } else {
                    let fields = fields
                        .iter()
                        .map(|f| (f.name().unwrap().deref(), f.ty().id()))
                        .collect();
                    Self::Struct(*idx, &vname, fields)
                }
            }
            _ => panic!("Only for enum variants"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "codec", derive(codec::Encode))]
pub enum TupleOrArray {
    Array(TypeId, u32),
    Tuple(Vec<TypeId>),
}
impl TupleOrArray {
    fn len(&self) -> usize {
        match self {
            Self::Array(_, len) => *len as usize,
            Self::Tuple(fields) => fields.len(),
        }
    }

    fn type_id(&self, i: usize) -> TypeId {
        match self {
            Self::Array(ty, _) => *ty,
            Self::Tuple(fields) => fields[i],
        }
    }
}
