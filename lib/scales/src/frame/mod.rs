//! Substrate frame runtime support.
//!
//! Provides metadata decoding, type compression, and SCALE cursor utilities
//! for working with Substrate frame-based chains.
//!
//! Enable with the `frame` feature flag.

pub(crate) mod cursor;
pub mod metadata;

#[cfg(feature = "compress")]
pub mod compress;
