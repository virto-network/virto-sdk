use sube::scales::{Registry, TypeDef, TypeId};

/// Produce a short human-readable description of a type.
pub fn describe(ty_id: TypeId, registry: &Registry) -> String {
    match registry.resolve(ty_id) {
        None => format!("?{ty_id}"),
        Some(td) => match td {
            TypeDef::Bool => "bool".into(),
            TypeDef::U8 => "u8".into(),
            TypeDef::U16 => "u16".into(),
            TypeDef::U32 => "u32".into(),
            TypeDef::U64 => "u64".into(),
            TypeDef::U128 => "u128".into(),
            TypeDef::I8 => "i8".into(),
            TypeDef::I16 => "i16".into(),
            TypeDef::I32 => "i32".into(),
            TypeDef::I64 => "i64".into(),
            TypeDef::I128 => "i128".into(),
            TypeDef::Char => "char".into(),
            TypeDef::Str => "String".into(),
            TypeDef::Bytes => "Vec<u8>".into(),
            TypeDef::Sequence(inner) => format!("Vec<{}>", describe(inner, registry)),
            TypeDef::Map(k, v) => {
                format!("Map<{}, {}>", describe(k, registry), describe(v, registry))
            }
            TypeDef::Array(inner, len) => format!("[{}; {len}]", describe(inner, registry)),
            TypeDef::Tuple(ids) => {
                let parts: Vec<String> = ids.iter().map(|id| describe(*id, registry)).collect();
                format!("({})", parts.join(", "))
            }
            TypeDef::StructUnit => "()".into(),
            TypeDef::StructNewType(inner) => describe(inner, registry),
            TypeDef::StructTuple(ids) => {
                let parts: Vec<String> = ids.iter().map(|id| describe(*id, registry)).collect();
                format!("({})", parts.join(", "))
            }
            TypeDef::Struct(fields) => {
                if fields.len() <= 3 {
                    let parts: Vec<String> = fields
                        .iter()
                        .map(|f| format!("{}: {}", f.name, describe(f.ty, registry)))
                        .collect();
                    format!("{{ {} }}", parts.join(", "))
                } else {
                    format!("struct({} fields)", fields.len())
                }
            }
            TypeDef::Variant(vdef) => {
                let count = vdef.variants().count();
                if count <= 4 {
                    let names: Vec<&str> = vdef.variants().map(|v| v.name()).collect();
                    names.join("|")
                } else {
                    format!("enum({count} variants)")
                }
            }
            TypeDef::Compact(inner) => format!("Compact<{}>", describe(inner, registry)),
            TypeDef::BitSequence(_, _) => "BitVec".into(),
        },
    }
}

/// Extract key type IDs from a map key, splitting tuples to match hashers.
pub fn extract_key_types(key_id: TypeId, num_hashers: usize, registry: &Registry) -> Vec<TypeId> {
    if num_hashers <= 1 {
        return vec![key_id];
    }
    match registry.resolve(key_id) {
        Some(TypeDef::Tuple(types) | TypeDef::StructTuple(types)) if types.len() == num_hashers => {
            types.to_vec()
        }
        _ => vec![key_id],
    }
}
