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

/// Abstraction for storage of private keys that are protected by some credentials.
/// A vault is a signer factory — it produces signers, but the wallet doesn't depend on it.
pub trait Vault {
    type Credentials;
    type Error;
    type Id;
    type Signer: crate::Signer;

    fn unlock(
        &mut self,
        account: Self::Id,
        cred: impl Into<Self::Credentials>,
    ) -> impl core::future::Future<Output = Result<Self::Signer, Self::Error>>;
}

pub(crate) mod utils {
    use crate::{any, any::AnySignature, Derive, Pair};

    /// The root account holds the master keypair from which child keys are derived.
    pub struct RootAccount {
        sub: crate::key_pair::sr25519::Pair,
    }

    impl core::fmt::Debug for RootAccount {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("RootAccount").field("key", &"<redacted>").finish()
        }
    }

    impl RootAccount {
        pub fn from_bytes(seed: &[u8]) -> Option<Self> {
            Some(RootAccount {
                sub: <crate::key_pair::sr25519::Pair as crate::Pair>::from_bytes(seed)?,
            })
        }

        pub fn derive(&self, path: &str) -> any::Pair {
            log::debug!("derive: {}", path);
            match path.get(..2) {
                Some("//") => self.sub.derive(path).into(),
                _ => self.sub.derive("//default").into(),
            }
        }
    }

    const MAX_PATH_LEN: usize = 16;

    /// A signer backed by a vault-derived keypair.
    pub struct DerivedSigner {
        pair: any::Pair,
        path: arrayvec::ArrayString<MAX_PATH_LEN>,
    }

    impl DerivedSigner {
        pub(crate) fn new(pair: any::Pair, path: &str) -> Self {
            let mut p = arrayvec::ArrayString::new();
            let len = path.len().min(MAX_PATH_LEN);
            let _ = p.try_push_str(&path[..len]);
            DerivedSigner { pair, path: p }
        }

        pub fn public(&self) -> impl crate::Public {
            self.pair.public()
        }
    }

    impl core::fmt::Debug for DerivedSigner {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("DerivedSigner").field("key", &"<redacted>").finish()
        }
    }

    impl crate::Signer for DerivedSigner {
        type Signature = AnySignature;

        fn account_id(&self) -> &str {
            &self.path
        }

        async fn sign_msg(&self, msg: impl AsRef<[u8]>) -> Result<Self::Signature, crate::SigningError> {
            self.pair.sign_msg(msg).await
        }

        async fn verify(&self, msg: impl AsRef<[u8]>, sig: impl AsRef<[u8]>) -> bool {
            self.pair.verify(msg, sig).await
        }
    }

    impl core::fmt::Display for DerivedSigner {
        fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            for byte in self.public().as_ref() {
                write!(f, "{:02x}", byte)?;
            }
            Ok(())
        }
    }
}
