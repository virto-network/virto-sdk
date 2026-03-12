use crate::registry::*;
use scale_info::{form::PortableForm, PortableRegistry};

type SiType = scale_info::Type<PortableForm>;
type SiTypeDef = scale_info::TypeDef<PortableForm>;
type SiComposite = scale_info::TypeDefComposite<PortableForm>;

/// Compress a `PortableRegistry` into a minimal `Registry`, stripping
/// docs, paths, and type params while pre-classifying types into
/// serde-compatible shapes.
pub fn compress(source: &PortableRegistry) -> Registry {
    let types = source
        .types
        .iter()
        .map(|pt| convert_type(&pt.ty, source))
        .collect();
    Registry::new(types)
}

fn convert_type(ty: &SiType, source: &PortableRegistry) -> TypeDef {
    use scale_info::TypeDefPrimitive as P;

    let name = || {
        ty.path
            .segments
            .last()
            .cloned()
            .unwrap_or_else(|| "".into())
    };

    let is_map = ty
        .path
        .segments
        .last()
        .map(|s| s == "BTreeMap")
        .unwrap_or(false);

    match &ty.type_def {
        SiTypeDef::Primitive(p) => match p {
            P::Bool => TypeDef::Bool,
            P::U8 => TypeDef::U8,
            P::U16 => TypeDef::U16,
            P::U32 => TypeDef::U32,
            P::U64 => TypeDef::U64,
            P::U128 => TypeDef::U128,
            P::I8 => TypeDef::I8,
            P::I16 => TypeDef::I16,
            P::I32 => TypeDef::I32,
            P::I64 => TypeDef::I64,
            P::I128 => TypeDef::I128,
            P::Char => TypeDef::Char,
            P::Str => TypeDef::Str,
            P::U256 | P::I256 => unimplemented!(),
        },
        SiTypeDef::Composite(c) => {
            if c.fields.is_empty() {
                TypeDef::StructUnit
            } else if is_map {
                let (k, v) = extract_map_types(c, source);
                TypeDef::Map(k, v)
            } else if c.fields.len() == 1 && c.fields[0].name.is_none() {
                TypeDef::StructNewType(c.fields[0].ty.id)
            } else if is_tuple(c) {
                TypeDef::StructTuple(c.fields.iter().map(|f| f.ty.id).collect())
            } else {
                TypeDef::Struct(
                    c.fields
                        .iter()
                        .map(|f| Field {
                            name: f.name.as_ref().unwrap().to_string(),
                            ty: f.ty.id,
                        })
                        .collect(),
                )
            }
        }
        SiTypeDef::Variant(v) => TypeDef::Variant(VariantDef {
            name: name(),
            variants: v
                .variants
                .iter()
                .map(|var| {
                    let fields = if var.fields.is_empty() {
                        Fields::Unit
                    } else if var.fields.len() == 1 && var.fields[0].name.is_none() {
                        Fields::NewType(var.fields[0].ty.id)
                    } else if var.fields[0].name.is_none() {
                        Fields::Tuple(var.fields.iter().map(|f| f.ty.id).collect())
                    } else {
                        Fields::Struct(
                            var.fields
                                .iter()
                                .map(|f| Field {
                                    name: f.name.as_ref().unwrap().to_string(),
                                    ty: f.ty.id,
                                })
                                .collect(),
                        )
                    };
                    Variant {
                        index: var.index,
                        name: var.name.to_string(),
                        fields,
                    }
                })
                .collect(),
        }),
        SiTypeDef::Sequence(s) => {
            let inner = s.type_param.id;
            if let Some(inner_ty) = source.resolve(inner) {
                if matches!(inner_ty.type_def, SiTypeDef::Primitive(P::U8)) {
                    return TypeDef::Bytes;
                }
            }
            TypeDef::Sequence(inner)
        }
        SiTypeDef::Array(a) => TypeDef::Array(a.type_param.id, a.len),
        SiTypeDef::Tuple(t) => TypeDef::Tuple(t.fields.iter().map(|f| f.id).collect()),
        SiTypeDef::Compact(c) => TypeDef::Compact(c.type_param.id),
        SiTypeDef::BitSequence(b) => TypeDef::BitSequence(b.bit_store_type.id, b.bit_order_type.id),
    }
}

fn is_tuple(c: &SiComposite) -> bool {
    c.fields.first().and_then(|f| f.name.as_ref()).is_none()
}

fn extract_map_types(c: &SiComposite, source: &PortableRegistry) -> (TypeId, TypeId) {
    let field = c.fields.first().expect("map field");
    let resolved = source.resolve(field.ty.id).unwrap();
    if let SiTypeDef::Sequence(s) = &resolved.type_def {
        let inner = source.resolve(s.type_param.id).unwrap();
        if let SiTypeDef::Tuple(t) = &inner.type_def {
            assert_eq!(t.fields.len(), 2);
            return (t.fields[0].id, t.fields[1].id);
        }
    }
    unreachable!()
}
