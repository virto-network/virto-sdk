use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Index into a [`Registry`].
pub type TypeId = u32;

// --- Internal compact types ---

#[derive(Debug, Clone, Copy)]
struct StrId(u32);

#[derive(Debug, Clone, Copy)]
struct Span(u32, u16);

#[derive(Debug, Clone)]
#[rustfmt::skip]
enum TDI {
    Bool, U8, U16, U32, U64, U128, I8, I16, I32, I64, I128, Char, Str, Bytes,
    Sequence(TypeId), Map(TypeId, TypeId), Array(TypeId, u32),
    Tuple(Span), StructUnit, StructNewType(TypeId), StructTuple(Span),
    Struct(Span), Variant(StrId, Span),
    Compact(TypeId), BitSequence(TypeId, TypeId),
}

#[derive(Debug, Clone)]
struct FI {
    name: StrId,
    ty: TypeId,
}

#[derive(Debug, Clone)]
struct VI {
    index: u8,
    name: StrId,
    fields: VFI,
}

#[derive(Debug, Clone)]
enum VFI {
    Unit,
    NewType(TypeId),
    Tuple(Span),
    Struct(Span),
}

// --- Registry ---

/// Arena-backed type registry. All strings, fields, variants, and type ID
/// lists are stored in contiguous slabs — no per-element heap allocations.
#[derive(Debug, Clone)]
pub struct Registry {
    types: Vec<TDI>,
    fields: Vec<FI>,
    variants: Vec<VI>,
    type_ids: Vec<TypeId>,
    strings: String,
    str_idx: Vec<(u32, u16)>,
}

impl Registry {
    pub fn new(types: Vec<TypeDefOwned>) -> Self {
        let mut r = Registry {
            types: Vec::with_capacity(types.len()),
            fields: Vec::new(),
            variants: Vec::new(),
            type_ids: Vec::new(),
            strings: String::new(),
            str_idx: Vec::new(),
        };
        for td in types {
            let c = r.compact(td);
            r.types.push(c);
        }
        r
    }

    #[inline]
    pub fn resolve(&self, id: TypeId) -> Option<TypeDef<'_>> {
        self.types.get(id as usize).map(|td| self.expand(td))
    }

    fn s(&self, id: StrId) -> &str {
        let (o, l) = self.str_idx[id.0 as usize];
        &self.strings[o as usize..o as usize + l as usize]
    }
    fn ids(&self, s: Span) -> &[TypeId] {
        &self.type_ids[s.0 as usize..s.0 as usize + s.1 as usize]
    }

    fn intern(&mut self, s: &str) -> StrId {
        for (i, &(o, l)) in self.str_idx.iter().enumerate() {
            if &self.strings[o as usize..o as usize + l as usize] == s {
                return StrId(i as u32);
            }
        }
        let id = StrId(self.str_idx.len() as u32);
        let o = self.strings.len() as u32;
        self.strings.push_str(s);
        self.str_idx.push((o, s.len() as u16));
        id
    }
    fn push_fs(&mut self, fs: Vec<FieldOwned>) -> Span {
        let start = self.fields.len() as u32;
        for f in &fs {
            let n = self.intern(&f.name);
            self.fields.push(FI { name: n, ty: f.ty });
        }
        Span(start, fs.len() as u16)
    }
    fn push_ids(&mut self, ids: Vec<TypeId>) -> Span {
        let start = self.type_ids.len() as u32;
        let len = ids.len() as u16;
        self.type_ids.extend(ids);
        Span(start, len)
    }
    fn push_vs(&mut self, vs: Vec<VariantOwned>) -> Span {
        let start = self.variants.len() as u32;
        let len = vs.len() as u16;
        for v in vs {
            let n = self.intern(&v.name);
            let f = match v.fields {
                FieldsOwned::Unit => VFI::Unit,
                FieldsOwned::NewType(id) => VFI::NewType(id),
                FieldsOwned::Tuple(ids) => VFI::Tuple(self.push_ids(ids)),
                FieldsOwned::Struct(fs) => VFI::Struct(self.push_fs(fs)),
            };
            self.variants.push(VI {
                index: v.index,
                name: n,
                fields: f,
            });
        }
        Span(start, len)
    }

    fn compact(&mut self, td: TypeDefOwned) -> TDI {
        use TypeDefOwned as O;
        match td {
            O::Bool => TDI::Bool,
            O::U8 => TDI::U8,
            O::U16 => TDI::U16,
            O::U32 => TDI::U32,
            O::U64 => TDI::U64,
            O::U128 => TDI::U128,
            O::I8 => TDI::I8,
            O::I16 => TDI::I16,
            O::I32 => TDI::I32,
            O::I64 => TDI::I64,
            O::I128 => TDI::I128,
            O::Char => TDI::Char,
            O::Str => TDI::Str,
            O::Bytes => TDI::Bytes,
            O::Sequence(id) => TDI::Sequence(id),
            O::Map(k, v) => TDI::Map(k, v),
            O::Array(id, n) => TDI::Array(id, n),
            O::Tuple(ids) => TDI::Tuple(self.push_ids(ids)),
            O::StructUnit => TDI::StructUnit,
            O::StructNewType(id) => TDI::StructNewType(id),
            O::StructTuple(ids) => TDI::StructTuple(self.push_ids(ids)),
            O::Struct(fs) => TDI::Struct(self.push_fs(fs)),
            O::Variant(v) => {
                let n = self.intern(&v.name);
                let vs = self.push_vs(v.variants);
                TDI::Variant(n, vs)
            }
            O::Compact(id) => TDI::Compact(id),
            O::BitSequence(s, o) => TDI::BitSequence(s, o),
        }
    }

    fn expand(&self, td: &TDI) -> TypeDef<'_> {
        match td {
            TDI::Bool => TypeDef::Bool,
            TDI::U8 => TypeDef::U8,
            TDI::U16 => TypeDef::U16,
            TDI::U32 => TypeDef::U32,
            TDI::U64 => TypeDef::U64,
            TDI::U128 => TypeDef::U128,
            TDI::I8 => TypeDef::I8,
            TDI::I16 => TypeDef::I16,
            TDI::I32 => TypeDef::I32,
            TDI::I64 => TypeDef::I64,
            TDI::I128 => TypeDef::I128,
            TDI::Char => TypeDef::Char,
            TDI::Str => TypeDef::Str,
            TDI::Bytes => TypeDef::Bytes,
            TDI::Sequence(id) => TypeDef::Sequence(*id),
            TDI::Map(k, v) => TypeDef::Map(*k, *v),
            TDI::Array(id, n) => TypeDef::Array(*id, *n),
            TDI::Tuple(s) => TypeDef::Tuple(self.ids(*s)),
            TDI::StructUnit => TypeDef::StructUnit,
            TDI::StructNewType(id) => TypeDef::StructNewType(*id),
            TDI::StructTuple(s) => TypeDef::StructTuple(self.ids(*s)),
            TDI::Struct(s) => TypeDef::Struct(StructFields(self, *s)),
            TDI::Variant(n, s) => TypeDef::Variant(VariantDef(self, *n, *s)),
            TDI::Compact(id) => TypeDef::Compact(*id),
            TDI::BitSequence(s, o) => TypeDef::BitSequence(*s, *o),
        }
    }
}

// --- Public view types ---

/// Resolved type definition — zero-cost view into the registry.
#[derive(Clone, Copy)]
#[rustfmt::skip]
pub enum TypeDef<'a> {
    Bool, U8, U16, U32, U64, U128, I8, I16, I32, I64, I128, Char, Str, Bytes,
    Sequence(TypeId), Map(TypeId, TypeId), Array(TypeId, u32),
    Tuple(&'a [TypeId]), StructUnit, StructNewType(TypeId), StructTuple(&'a [TypeId]),
    Struct(StructFields<'a>), Variant(VariantDef<'a>),
    Compact(TypeId), BitSequence(TypeId, TypeId),
}

impl core::fmt::Debug for TypeDef<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::Bool => write!(f, "Bool"),
            Self::U8 => write!(f, "U8"),
            Self::U16 => write!(f, "U16"),
            Self::U32 => write!(f, "U32"),
            Self::U64 => write!(f, "U64"),
            Self::U128 => write!(f, "U128"),
            Self::I8 => write!(f, "I8"),
            Self::I16 => write!(f, "I16"),
            Self::I32 => write!(f, "I32"),
            Self::I64 => write!(f, "I64"),
            Self::I128 => write!(f, "I128"),
            Self::Char => write!(f, "Char"),
            Self::Str => write!(f, "Str"),
            Self::Bytes => write!(f, "Bytes"),
            Self::Sequence(id) => write!(f, "Sequence({id})"),
            Self::Map(k, v) => write!(f, "Map({k},{v})"),
            Self::Array(id, n) => write!(f, "Array({id},{n})"),
            Self::Tuple(ids) => write!(f, "Tuple({})", ids.len()),
            Self::StructUnit => write!(f, "StructUnit"),
            Self::StructNewType(id) => write!(f, "StructNewType({id})"),
            Self::StructTuple(ids) => write!(f, "StructTuple({})", ids.len()),
            Self::Struct(s) => write!(f, "Struct({})", s.len()),
            Self::Variant(v) => write!(f, "Variant({})", v.name()),
            Self::Compact(id) => write!(f, "Compact({id})"),
            Self::BitSequence(s, o) => write!(f, "BitSequence({s},{o})"),
        }
    }
}

/// View over struct fields.
#[derive(Clone, Copy)]
pub struct StructFields<'a>(&'a Registry, Span);

impl<'a> StructFields<'a> {
    pub fn iter(&self) -> impl Iterator<Item = Field<'a>> + 'a {
        let reg = self.0;
        let s = self.1;
        reg.fields[s.0 as usize..s.0 as usize + s.1 as usize]
            .iter()
            .map(move |f| Field {
                name: reg.s(f.name),
                ty: f.ty,
            })
    }
    pub fn len(&self) -> usize {
        self.1 .1 as usize
    }
    pub fn is_empty(&self) -> bool {
        self.1 .1 == 0
    }
}

impl<'a> IntoIterator for StructFields<'a> {
    type Item = Field<'a>;
    type IntoIter = alloc::vec::IntoIter<Field<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter().collect::<Vec<_>>().into_iter()
    }
}

/// A field with borrowed name.
#[derive(Debug, Clone, Copy)]
pub struct Field<'a> {
    pub name: &'a str,
    pub ty: TypeId,
}

/// View over an enum definition.
#[derive(Clone, Copy)]
pub struct VariantDef<'a>(&'a Registry, StrId, Span);

impl<'a> VariantDef<'a> {
    pub fn name(&self) -> &'a str {
        self.0.s(self.1)
    }

    pub fn variants(&self) -> VariantIter<'a> {
        let s = self.2;
        VariantIter {
            reg: self.0,
            inner: self.0.variants[s.0 as usize..s.0 as usize + s.1 as usize].iter(),
        }
    }

    pub fn variant(&self, index: u8) -> Result<Variant<'a>, crate::Error> {
        self.variants()
            .find(|v| v.index() == index)
            .ok_or(crate::Error::InvalidVariant(index))
    }
}

/// Iterator over variants.
pub struct VariantIter<'a> {
    reg: &'a Registry,
    inner: core::slice::Iter<'a, VI>,
}

impl<'a> Iterator for VariantIter<'a> {
    type Item = Variant<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|v| Variant {
            reg: self.reg,
            i: v,
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a> ExactSizeIterator for VariantIter<'a> {}

/// A single enum variant.
#[derive(Clone, Copy)]
pub struct Variant<'a> {
    reg: &'a Registry,
    i: &'a VI,
}

impl<'a> Variant<'a> {
    pub fn index(&self) -> u8 {
        self.i.index
    }
    pub fn name(&self) -> &'a str {
        self.reg.s(self.i.name)
    }
    pub fn fields(&self) -> Fields<'a> {
        match &self.i.fields {
            VFI::Unit => Fields::Unit,
            VFI::NewType(id) => Fields::NewType(*id),
            VFI::Tuple(s) => Fields::Tuple(self.reg.ids(*s)),
            VFI::Struct(s) => Fields::Struct(StructFields(self.reg, *s)),
        }
    }
}

/// Variant payload shape.
pub enum Fields<'a> {
    Unit,
    NewType(TypeId),
    Tuple(&'a [TypeId]),
    Struct(StructFields<'a>),
}

// --- Owning types (for construction) ---

#[rustfmt::skip]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeDefOwned {
    Bool, U8, U16, U32, U64, U128, I8, I16, I32, I64, I128, Char, Str, Bytes,
    Sequence(TypeId), Map(TypeId, TypeId), Array(TypeId, u32),
    Tuple(Vec<TypeId>), StructUnit, StructNewType(TypeId), StructTuple(Vec<TypeId>),
    Struct(Vec<FieldOwned>), Variant(VariantDefOwned),
    Compact(TypeId), BitSequence(TypeId, TypeId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldOwned {
    pub name: String,
    pub ty: TypeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantDefOwned {
    pub name: String,
    pub variants: Vec<VariantOwned>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantOwned {
    pub index: u8,
    pub name: String,
    pub fields: FieldsOwned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldsOwned {
    Unit,
    NewType(TypeId),
    Tuple(Vec<TypeId>),
    Struct(Vec<FieldOwned>),
}
