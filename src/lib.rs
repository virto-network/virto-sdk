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

#[cfg(feature = "experimental-serializer")]
pub use serializer::{to_bytes, to_bytes_with_info, to_vec, to_vec_with_info, Serializer};
pub use value::Value;

use prelude::*;
use scale_info::{Field, MetaType, Type, Variant};

mod prelude {
    pub use alloc::{
        collections::BTreeMap,
        string::{String, ToString},
        vec::Vec,
    };
}

macro_rules! is_tuple {
    ($it:ident) => {
        $it.fields().first().and_then(Field::name).is_none()
    };
}

/// A convenient representation of the scale-info types to a format
/// that matches serde model more closely
#[rustfmt::skip]
#[derive(Debug, Clone)]
pub enum SerdeType {
    Bool,
    U8, U16, U32, U64, U128,
    I8, I16, I32, I64, I128,
    Bytes,
    Char,
    Str,
    Sequence(Type),
    Map(Type, Type),
    Tuple(TupleOrArray),
    Struct(Vec<Field>), StructUnit, StructNewType(Type), StructTuple(Vec<Field>),
    Variant(String, Vec<Variant>, Option<u8>),
}

impl From<&mut Type> for SerdeType {
    fn from(ty: &mut Type) -> Self {
        ty.clone().into()
    }
}

impl From<&Type> for SerdeType {
    fn from(ty: &Type) -> Self {
        ty.clone().into()
    }
}

impl From<Type> for SerdeType {
    fn from(ty: Type) -> Self {
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
                } else if is_tuple!(c) {
                    Self::StructTuple(fields.into())
                } else {
                    Self::Struct(fields.into())
                }
            }
            Variant(v) => Self::Variant(name.into(), v.variants().into(), None),
            Sequence(s) => {
                let ty = s.type_param().type_info();
                if ty.path().segments() != &["u8"] {
                    Self::Sequence(ty)
                } else {
                    Self::Bytes
                }
            }
            Array(a) => Self::Tuple(TupleOrArray::Array(a.type_param().type_info(), a.len())),
            Tuple(t) => Self::Tuple(TupleOrArray::Tuple(
                t.fields().iter().map(MetaType::type_info).collect(),
            )),
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

// Utilities for enum variants
impl SerdeType {
    fn pick(&self, index: u8) -> Self {
        match self {
            SerdeType::Variant(name, variant, Some(_)) => {
                Self::Variant(name.to_string(), variant.to_vec(), Some(index))
            }
            SerdeType::Variant(name, variants, None) => {
                let v = variants.iter().find(|v| v.index() == index).unwrap();
                Self::Variant(name.clone(), vec![v.clone()], Some(index))
            }
            _ => panic!("Only for enum variants"),
        }
    }

    #[cfg(feature = "experimental-serializer")]
    fn pick_mut<F, A, B>(&mut self, selection: A, get_field: F) -> &Self
    where
        F: Fn(&Variant) -> B,
        A: AsRef<[u8]> + PartialEq + core::fmt::Debug,
        B: AsRef<[u8]> + PartialEq + core::fmt::Debug,
    {
        match self {
            SerdeType::Variant(_, _, Some(_)) => self,
            SerdeType::Variant(_, ref mut variants, idx @ None) => {
                let i = variants
                    .iter()
                    .map(|v| get_field(v))
                    .position(|f| f.as_ref() == selection.as_ref())
                    .expect("index") as u8;
                variants.retain(|v| v.index() == i);
                *idx = Some(i);
                self
            }
            _ => panic!("Only for enum variants"),
        }
    }

    #[cfg(feature = "experimental-serializer")]
    fn variant_id(&self) -> u8 {
        match self {
            SerdeType::Variant(_, _, Some(id)) => *id,
            _ => panic!("Only for enum variants"),
        }
    }
}

#[derive(Debug)]
enum EnumVariant<'a> {
    OptionNone,
    OptionSome(Type),
    Unit(u8, &'a str),
    NewType(u8, &'a str, Type),
    Tuple(u8, &'a str, Vec<Type>),
    Struct(u8, &'a str, Vec<(&'a str, Type)>),
}

impl<'a> From<&SerdeType> for EnumVariant<'a> {
    fn from(ty: &SerdeType) -> Self {
        match ty {
            SerdeType::Variant(name, variants, Some(idx)) => {
                let variant = variants.first().expect("single variant");
                let fields = variant.fields();
                let vname = *variant.name();

                if fields.is_empty() {
                    if name == "Option" && vname == "None" {
                        Self::OptionNone
                    } else {
                        Self::Unit(*idx, vname)
                    }
                } else if is_tuple!(variant) {
                    if fields.len() == 1 {
                        let ty = fields.first().map(|f| f.ty().type_info()).unwrap();
                        return if name == "Option" && variant.name() == &"Some" {
                            Self::OptionSome(ty)
                        } else {
                            Self::NewType(*idx, vname, ty)
                        };
                    } else {
                        let fields = fields.iter().map(|f| f.ty().type_info()).collect();
                        Self::Tuple(*idx, vname, fields)
                    }
                } else {
                    let fields = fields
                        .iter()
                        .map(|f| (*f.name().unwrap(), f.ty().type_info()))
                        .collect();
                    Self::Struct(*idx, vname, fields)
                }
            }
            _ => panic!("Only for enum variants"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TupleOrArray {
    Array(Type, u32),
    Tuple(Vec<Type>),
}
impl TupleOrArray {
    fn len(&self) -> usize {
        match self {
            Self::Array(_, len) => *len as usize,
            Self::Tuple(fields) => fields.len(),
        }
    }

    fn type_info(&self, i: usize) -> &Type {
        match self {
            Self::Array(ty, _) => ty,
            Self::Tuple(fields) => &fields[i],
        }
    }
}
