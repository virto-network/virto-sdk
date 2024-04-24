//! Collection of supported Vault backends
#[cfg(feature = "vault_os")]
mod os;
// #[cfg(feature = "vault_pass")]
mod pass;
mod simple;

#[cfg(feature = "vault_os")]
pub use os::*;
#[cfg(feature = "vault_pass")]
pub use pass::*;
pub use simple::*;

use crate::{any, key_pair, Derive, Public, Signer};



/// Abstraction for storage of private keys that are protected by some credentials.
pub trait Vault {
    type Credentials;
    type Error;
    type Account<'a>;
    type Signer: Signer;

    async fn unlock<'a>(
        &mut self,
        account: Self::Account<'a>,
        cred: impl Into<Self::Credentials>,
    ) -> Result<Self::Signer, Self::Error>;
}

/// The root account is a container of the key pairs stored in the vault and cannot be
/// used to sign messages directly, we always derive new key pairs from it to create
/// and use accounts with the wallet.
#[derive(Debug)]
pub struct RootAccount {
    #[cfg(feature = "substrate")]
    sub: crate::key_pair::sr25519::Pair,
}

impl RootAccount {
    fn from_bytes(seed: &[u8]) -> Self {
        #[cfg(not(feature = "substrate"))]
        let _ = seed;
        RootAccount {
            #[cfg(feature = "substrate")]
            sub: <crate::key_pair::sr25519::Pair as crate::Pair>::from_bytes(seed),
        }
    }
}

impl<'a> Derive for &'a RootAccount {
    type Pair = any::Pair;

    fn derive(&self, path: &str) -> Self::Pair
    where
        Self: Sized,
    {
        match &path[..2] {
            #[cfg(feature = "substrate")]
            "//" => self.sub.derive(path).into(),
            "m/" => unimplemented!(),
            #[cfg(feature = "substrate")]
            _ => self.sub.derive("//default").into(),
            #[cfg(not(feature = "substrate"))]
            _ => unreachable!(),
        }
    }
}

