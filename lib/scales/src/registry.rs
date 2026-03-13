use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Index into a [`Registry`].
pub type TypeId = u32;

/// A minimal type registry storing only what is needed for SCALE serialization.
/// No docs, no full paths, no type params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry(Vec<TypeDef>);

impl Registry {
    /// Create a registry from a list of type definitions.
    pub fn new(types: Vec<TypeDef>) -> Self {
        Self(types)
    }

    /// Look up a type by its ID.
    #[inline]
    #[must_use]
    pub fn resolve(&self, id: TypeId) -> Option<&TypeDef> {
        self.0.get(id as usize)
    }
}

/// Type definitions that map directly to serde's data model.
#[rustfmt::skip]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeDef {
    Bool,
    U8, U16, U32, U64, U128,
    I8, I16, I32, I64, I128,
    Char,
    Str,
    /// `Vec<u8>` serialized as raw bytes
    Bytes,
    /// Homogeneous sequence with compact-length prefix
    Sequence(TypeId),
    /// `BTreeMap<K, V>`
    Map(TypeId, TypeId),
    /// Fixed-length array `[T; N]`
    Array(TypeId, u32),
    /// Heterogeneous tuple `(T1, T2, ...)`
    Tuple(Vec<TypeId>),
    /// Unit struct (zero fields)
    StructUnit,
    /// Newtype struct `Foo(T)`
    StructNewType(TypeId),
    /// Tuple struct `Foo(T1, T2, ...)`
    StructTuple(Vec<TypeId>),
    /// Named-field struct
    Struct(Vec<Field>),
    /// Enum type
    Variant(VariantDef),
    /// Compact-encoded integer
    Compact(TypeId),
    /// Bit sequence (store, order type IDs)
    BitSequence(TypeId, TypeId),
}

/// A named field within a struct or variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub ty: TypeId,
}

/// Definition of an enum type with its variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantDef {
    /// Short name (e.g. "Option") used for special-case detection.
    pub name: String,
    pub variants: Vec<Variant>,
}

impl VariantDef {
    /// Find a variant by its SCALE index byte.
    pub fn variant(&self, index: u8) -> Result<&Variant, crate::Error> {
        self.variants
            .iter()
            .find(|v| v.index == index)
            .ok_or(crate::Error::InvalidVariant(index))
    }
}

/// A single variant of an enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    pub index: u8,
    pub name: String,
    pub fields: Fields,
}

/// Pre-classified variant payload shapes matching serde's enum model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Fields {
    Unit,
    NewType(TypeId),
    Tuple(Vec<TypeId>),
    Struct(Vec<Field>),
}
