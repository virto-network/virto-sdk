//! Lightweight dynamic value type for extrinsic extensions.
//!
//! Replaces `serde_json::Value` for the small set of values needed
//! by signed extension encoding. Implements `Serialize` so `scales`
//! can SCALE-encode it via `to_vec_with_info`.

use alloc::string::String;
use alloc::vec::Vec;
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;

/// A simple dynamic value — enough for extension defaults and call bodies.
#[derive(Debug, Clone, PartialEq)]
pub enum DynValue {
    Null,
    Bool(bool),
    U32(u32),
    U64(u64),
    Str(String),
    Map(Vec<(String, DynValue)>),
}

impl Serialize for DynValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            DynValue::Null => serializer.serialize_none(),
            DynValue::Bool(b) => serializer.serialize_bool(*b),
            DynValue::U32(n) => serializer.serialize_u32(*n),
            DynValue::U64(n) => serializer.serialize_u64(*n),
            DynValue::Str(s) => serializer.serialize_str(s),
            DynValue::Map(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

/// Convenience constructors (replace `serde_json::json!` macros).
impl DynValue {
    pub fn obj(entries: &[(&str, DynValue)]) -> Self {
        DynValue::Map(
            entries
                .iter()
                .map(|(k, v)| ((*k).into(), v.clone()))
                .collect(),
        )
    }
}

impl From<u32> for DynValue {
    fn from(n: u32) -> Self {
        DynValue::U32(n)
    }
}

impl From<u64> for DynValue {
    fn from(n: u64) -> Self {
        DynValue::U64(n)
    }
}

impl From<&str> for DynValue {
    fn from(s: &str) -> Self {
        DynValue::Str(s.into())
    }
}

impl From<String> for DynValue {
    fn from(s: String) -> Self {
        DynValue::Str(s)
    }
}

impl DynValue {
    pub fn as_object(&self) -> Option<&Vec<(String, DynValue)>> {
        match self {
            DynValue::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            DynValue::U32(n) => Some(*n as u64),
            DynValue::U64(n) => Some(*n),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&DynValue> {
        match self {
            DynValue::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// Convert from `scales::Value` (decoded SCALE data) to `DynValue`.
///
/// Supports primitives and flat structs. Used for introspecting
/// decoded chain constants (e.g. System::Version).
impl<'a> TryFrom<scales::Value<'a>> for DynValue {
    type Error = &'static str;

    fn try_from(v: scales::Value<'a>) -> Result<Self, Self::Error> {
        if let Some(n) = v.as_u64() {
            return Ok(DynValue::U64(n));
        }
        if let Some(n) = v.as_u32() {
            return Ok(DynValue::U32(n));
        }
        if let Some(s) = v.as_str() {
            return Ok(DynValue::Str(s.into()));
        }
        if let Some(b) = v.as_bool() {
            return Ok(DynValue::Bool(b));
        }
        // Struct/composite → Map
        if let Some(iter) = v.fields_iter() {
            let mut entries = Vec::new();
            for (name, field) in iter {
                let val = DynValue::try_from(field).unwrap_or(DynValue::Null);
                entries.push((name.into(), val));
            }
            return Ok(DynValue::Map(entries));
        }
        Err("unsupported scales::Value variant")
    }
}

impl From<i32> for DynValue {
    fn from(n: i32) -> Self {
        if n >= 0 {
            DynValue::U32(n as u32)
        } else {
            // Negative values shouldn't appear in SCALE extensions
            DynValue::U32(0)
        }
    }
}
