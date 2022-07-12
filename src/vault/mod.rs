//! Collection of supported Vault backends
#[cfg(feature = "vault_os")]
mod os;
#[cfg(feature = "vault_simple")]
mod simple;

#[cfg(feature = "vault_os")]
pub use os::*;
#[cfg(feature = "vault_simple")]
pub use simple::*;
