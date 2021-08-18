#![cfg_attr(not(feature = "std"), no_std)]
///! # Scales
///!
///! Dynamic SCALE Serialization using `scale-info` type information.
///!
mod serializer;
mod value;

pub use serializer::{to_bytes, Serializer};
pub use value::Value;
