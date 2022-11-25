//! Collection of supported Vault backends
#[cfg(feature = "vault_os")]
mod os;
#[cfg(feature = "vault_pass")]
mod pass;
#[cfg(feature = "vault_simple")]
mod simple;

#[cfg(feature = "vault_os")]
pub use os::*;
#[cfg(feature = "vault_pass")]
pub use pass::*;
#[cfg(feature = "vault_simple")]
pub use simple::*;
