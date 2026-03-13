#![cfg_attr(not(feature = "std"), no_std)]
//!
//! # Scales
//!
//! Dynamic SCALE Serialization using `scale-info` type information.

#[cfg_attr(feature = "serializer", macro_use)]
extern crate alloc;

#[cfg(feature = "scale-info")]
pub mod compress;
pub mod error;
pub mod registry;
#[cfg(feature = "serializer")]
mod serializer;
mod value;

pub use error::Error;
pub use registry::{Registry, TypeDef, TypeId};
#[cfg(feature = "json")]
pub use serde_json::Value as JsonValue;
#[cfg(feature = "serializer")]
pub use serializer::{to_bytes, to_bytes_with_info, to_vec, to_vec_with_info, Serializer};
#[cfg(all(feature = "serializer", feature = "json"))]
pub use serializer::{to_bytes_from_iter, to_vec_from_iter};
pub use value::{Cursor, FieldIter, SeqIter, TupleIter, Value};

mod prelude {
    pub use alloc::string::{String, ToString};
    #[cfg(feature = "serializer")]
    pub use alloc::vec::Vec;
}

// adapted from https://github.com/paritytech/parity-scale-codec/blob/master/src/compact.rs#L336
#[allow(clippy::all)]
pub(crate) fn compact_encode(n: u128, mut dest: impl bytes::BufMut) {
    match n {
        0..=0b0011_1111 => dest.put_u8((n as u8) << 2),
        0..=0b0011_1111_1111_1111 => dest.put_u16_le(((n as u16) << 2) | 0b01),
        0..=0b0011_1111_1111_1111_1111_1111_1111_1111 => dest.put_u32_le(((n as u32) << 2) | 0b10),
        _ => {
            let bytes_needed = 16 - n.leading_zeros() / 8;
            assert!(bytes_needed >= 4);
            dest.put_u8(0b11 + ((bytes_needed - 4) << 2) as u8);
            let mut v = n;
            for _ in 0..bytes_needed {
                dest.put_u8(v as u8);
                v >>= 8;
            }
        }
    }
}

#[cfg(test)]
mod tests;
