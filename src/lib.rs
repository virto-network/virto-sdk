#![cfg_attr(not(feature = "std"), no_std)]
///! # Scales
///!
///! Dynamic SCALE Serialization using `scale-info` type information.
///!

#[cfg(feature = "serializer")]
mod serializer;
mod value;

#[cfg(feature = "serializer")]
pub use serializer::{to_writer, Serializer};
pub use value::Value;
