//! Substrate frame runtime support.
//!
//! Provides metadata decoding, type compression, and SCALE cursor utilities
//! for working with Substrate frame-based chains.
//!
//! Enable with the `frame` feature flag.

/// Low-level SCALE binary cursor for frame decoders.
pub mod cursor;
pub mod extrinsic;
pub mod metadata;
#[cfg(feature = "async")]
pub mod stream_cursor;
#[cfg(feature = "async")]
pub mod streaming_metadata;

#[cfg(any(feature = "compress", test))]
pub mod compress;
