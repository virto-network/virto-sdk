//! Transport backends for hardware wallets and remote signers.
//!
//! Each transport provides a ready-made [`ProxySigner`](crate::ProxySigner)
//! that handles the device communication protocol.

#[cfg(feature = "ledger")]
pub mod ledger;

#[cfg(feature = "trezor")]
pub mod trezor;
