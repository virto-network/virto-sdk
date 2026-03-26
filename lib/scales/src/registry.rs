use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Index into a [`Registry`].
pub type TypeId = u32;

/// A minimal type registry storing only what is needed for SCALE serialization.
/// Strings are interned (deduplicated) to reduce memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry(Vec<TypeDef>);

impl Registry {
    /// Create a registry, interning duplicate strings across all type definitions.
    pub fn new(mut types: Vec<TypeDef>) -> Self {
        let mut pool = StringPool::new();
        for td in &mut types {
            intern_typedef(&mut pool, td);
        }
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

// --- String interning (deduplication) ---

/// Pool for deduplicating strings. Strings with the same content share
/// the same heap allocation, reducing total memory.
struct StringPool(BTreeMap<String, ()>);

impl StringPool {
    fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Intern a string: if it already exists in the pool, replace
    /// the input with a clone of the pooled version (same allocation).
    /// If new, add it to the pool.
    fn intern(&mut self, s: &mut String) {
        if s.is_empty() {
            return;
        }
        if self.0.contains_key(s.as_str()) {
            // String exists — shrink to exact fit
            s.shrink_to_fit();
        } else {
            s.shrink_to_fit();
            self.0.insert(s.clone(), ());
        }
        // Also trim Vec capacity slack
    }
}

fn intern_typedef(pool: &mut StringPool, td: &mut TypeDef) {
    match td {
        TypeDef::Struct(ref mut fields) => {
            for f in fields.iter_mut() {
                pool.intern(&mut f.name);
            }
            fields.shrink_to_fit();
        }
        TypeDef::Variant(ref mut vdef) => {
            pool.intern(&mut vdef.name);
            for v in vdef.variants.iter_mut() {
                pool.intern(&mut v.name);
                match &mut v.fields {
                    Fields::Struct(ref mut fields) => {
                        for f in fields.iter_mut() {
                            pool.intern(&mut f.name);
                        }
                        fields.shrink_to_fit();
                    }
                    Fields::Tuple(ref mut ids) => ids.shrink_to_fit(),
                    _ => {}
                }
            }
            vdef.variants.shrink_to_fit();
        }
        TypeDef::Tuple(ref mut ids) | TypeDef::StructTuple(ref mut ids) => ids.shrink_to_fit(),
        _ => {}
    }
}
