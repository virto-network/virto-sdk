//! Collection of supported Vault backends
#[cfg(feature = "vault_os")]
mod os;
#[cfg(feature = "vault_pass")]
mod pass;
mod simple;

#[cfg(feature = "vault_os")]
pub use os::*;
#[cfg(feature = "vault_pass")]
pub use pass::*;
pub use simple::*;

use crate::{any, Derive};

/// Abstration for storage of private keys that are protected by some credentials.
pub trait Vault {
    type Credentials;
    type Error;

    /// Use a set of credentials to make the guarded keys available to the user.
    /// It returns a `Future` to allow for vaults that might take an arbitrary amount
    /// of time getting the secret ready like waiting for some user physical interaction.
    async fn unlock<T>(
        &mut self,
        cred: impl Into<Self::Credentials>,
        cb: impl FnMut(&RootAccount) -> T,
    ) -> Result<T, Self::Error>;
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

macro_rules! seed_from_entropy {
    ($seed: ident, $pin: expr) => {
        #[cfg(feature = "util_pin")]
        let protected_seed = $pin.protect::<64>($seed);
        #[cfg(feature = "util_pin")]
        let $seed = &protected_seed;
        #[cfg(not(feature = "util_pin"))]
        let _ = &$pin; // use the variable to avoid warning
    };
}
pub(crate) use seed_from_entropy;
