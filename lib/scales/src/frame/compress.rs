use alloc::string::ToString;
use crate::registry::*;
use crate::Error;
use scale_info::{form::PortableForm, PortableRegistry};

type SiType = scale_info::Type<PortableForm>;
type SiTypeDef = scale_info::TypeDef<PortableForm>;
type SiComposite = scale_info::TypeDefComposite<PortableForm>;

/// Compress a `PortableRegistry` into a minimal `Registry`, stripping
/// docs, paths, and type params while pre-classifying types into
/// serde-compatible shapes.
pub fn compress(source: &PortableRegistry) -> Result<Registry, Error> {
    let types = source
        .types
        .iter()
        .map(|pt| convert_type(&pt.ty, source))
        .collect::<Result<_, _>>()?;
    Ok(Registry::new(types))
}

fn convert_type(ty: &SiType, source: &PortableRegistry) -> Result<TypeDef, Error> {
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
        .is_some_and(|s| s == "BTreeMap");

    Ok(match &ty.type_def {
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
            P::U256 | P::I256 => return Err(Error::BadInput("256-bit integers not supported".into())),
        },
        SiTypeDef::Composite(c) => {
            if c.fields.is_empty() {
                TypeDef::StructUnit
            } else if is_map {
                let (k, v) = extract_map_types(c, source)?;
                TypeDef::Map(k, v)
            } else if c.fields.len() == 1 && c.fields[0].name.is_none() {
                TypeDef::StructNewType(c.fields[0].ty.id)
            } else if is_tuple(c) {
                TypeDef::StructTuple(c.fields.iter().map(|f| f.ty.id).collect())
            } else {
                TypeDef::Struct(
                    c.fields
                        .iter()
                        .map(|f| Ok(Field {
                            name: f.name.as_ref().ok_or(Error::BadInput("expected named field".into()))?.to_string(),
                            ty: f.ty.id,
                        }))
                        .collect::<Result<_, Error>>()?,
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
                                .map(|f| Ok(Field {
                                    name: f.name.as_ref().ok_or(Error::BadInput("expected named field".into()))?.to_string(),
                                    ty: f.ty.id,
                                }))
                                .collect::<Result<_, Error>>()?,
                        )
                    };
                    Ok(Variant {
                        index: var.index,
                        name: var.name.to_string(),
                        fields,
                    })
                })
                .collect::<Result<_, Error>>()?,
        }),
        SiTypeDef::Sequence(s) => {
            let inner = s.type_param.id;
            if let Some(inner_ty) = source.resolve(inner) {
                if matches!(inner_ty.type_def, SiTypeDef::Primitive(P::U8)) {
                    return Ok(TypeDef::Bytes);
                }
            }
            TypeDef::Sequence(inner)
        }
        SiTypeDef::Array(a) => TypeDef::Array(a.type_param.id, a.len),
        SiTypeDef::Tuple(t) => TypeDef::Tuple(t.fields.iter().map(|f| f.id).collect()),
        SiTypeDef::Compact(c) => TypeDef::Compact(c.type_param.id),
        SiTypeDef::BitSequence(b) => TypeDef::BitSequence(b.bit_store_type.id, b.bit_order_type.id),
    })
}

/// Compress only the types reachable from `root_ids`, producing a smaller
/// registry with remapped (contiguous) type IDs.
/// Returns `(registry, id_map)` where `id_map[old_id] = new_id`.
pub fn compress_filtered(
    source: &PortableRegistry,
    root_ids: &[u32],
) -> Result<(Registry, alloc::vec::Vec<Option<u32>>), Error> {
    use alloc::collections::BTreeSet;

    // Walk all reachable types from roots
    let mut visited = BTreeSet::new();
    let mut stack: alloc::vec::Vec<u32> = root_ids.to_vec();
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(ty) = source.resolve(id) {
            collect_type_refs(&ty.type_def, &mut stack);
        }
    }

    // Build old→new mapping
    let max_id = source.types.len() as u32;
    let mut id_map: alloc::vec::Vec<Option<u32>> = alloc::vec![None; max_id as usize];
    let mut new_id = 0u32;
    for &old_id in &visited {
        if (old_id as usize) < id_map.len() {
            id_map[old_id as usize] = Some(new_id);
            new_id += 1;
        }
    }

    // Convert only visited types with remapped IDs
    let types: Result<alloc::vec::Vec<TypeDef>, Error> = visited
        .iter()
        .map(|&old_id| {
            let ty = source
                .resolve(old_id)
                .ok_or(Error::BadInput("missing type in filtered set".into()))?;
            let mut td = convert_type(&ty, source)?;
            remap_type_ids(&mut td, &id_map);
            Ok(td)
        })
        .collect();

    Ok((Registry::new(types?), id_map))
}

fn collect_type_refs(def: &SiTypeDef, out: &mut alloc::vec::Vec<u32>) {
    match def {
        SiTypeDef::Primitive(_) => {}
        SiTypeDef::Composite(c) => {
            for f in &c.fields {
                out.push(f.ty.id);
            }
        }
        SiTypeDef::Variant(v) => {
            for var in &v.variants {
                for f in &var.fields {
                    out.push(f.ty.id);
                }
            }
        }
        SiTypeDef::Sequence(s) => out.push(s.type_param.id),
        SiTypeDef::Array(a) => out.push(a.type_param.id),
        SiTypeDef::Tuple(t) => {
            for f in &t.fields {
                out.push(f.id);
            }
        }
        SiTypeDef::Compact(c) => out.push(c.type_param.id),
        SiTypeDef::BitSequence(b) => {
            out.push(b.bit_store_type.id);
            out.push(b.bit_order_type.id);
        }
    }
}

fn remap_type_ids(td: &mut TypeDef, id_map: &[Option<u32>]) {
    fn remap(id: &mut TypeId, map: &[Option<u32>]) {
        if let Some(new) = map.get(*id as usize).copied().flatten() {
            *id = new;
        }
    }

    match td {
        TypeDef::Sequence(id) | TypeDef::StructNewType(id) | TypeDef::Compact(id) => {
            remap(id, id_map);
        }
        TypeDef::Map(k, v) | TypeDef::BitSequence(k, v) => {
            remap(k, id_map);
            remap(v, id_map);
        }
        TypeDef::Array(id, _) => remap(id, id_map),
        TypeDef::Tuple(ids) | TypeDef::StructTuple(ids) => {
            for id in ids { remap(id, id_map); }
        }
        TypeDef::Struct(fields) => {
            for f in fields { remap(&mut f.ty, id_map); }
        }
        TypeDef::Variant(vdef) => {
            for v in &mut vdef.variants {
                match &mut v.fields {
                    Fields::NewType(id) => remap(id, id_map),
                    Fields::Tuple(ids) => { for id in ids { remap(id, id_map); } }
                    Fields::Struct(fields) => { for f in fields { remap(&mut f.ty, id_map); } }
                    Fields::Unit => {}
                }
            }
        }
        _ => {} // primitives, Bytes, StructUnit
    }
}

fn is_tuple(c: &SiComposite) -> bool {
    c.fields.first().and_then(|f| f.name.as_ref()).is_none()
}

fn extract_map_types(c: &SiComposite, source: &PortableRegistry) -> Result<(TypeId, TypeId), Error> {
    let field = c.fields.first().ok_or(Error::BadInput("map has no fields".into()))?;
    let resolved = source.resolve(field.ty.id).ok_or(Error::BadInput("unresolved map type".into()))?;
    if let SiTypeDef::Sequence(s) = &resolved.type_def {
        let inner = source.resolve(s.type_param.id).ok_or(Error::BadInput("unresolved map inner type".into()))?;
        if let SiTypeDef::Tuple(t) = &inner.type_def {
            if t.fields.len() == 2 {
                return Ok((t.fields[0].id, t.fields[1].id));
            }
        }
    }
    Err(Error::BadInput("unexpected map structure".into()))
}
