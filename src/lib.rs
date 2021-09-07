#![cfg_attr(not(feature = "std"), no_std)]
///! # Scales
///!
///! Dynamic SCALE Serialization using `scale-info` type information.
///!
mod value;

pub use value::Value;
