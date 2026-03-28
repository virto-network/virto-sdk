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

use crate::account::Account;

/// Abstraction for storage of private keys that are protected by some credentials.
pub trait Vault {
    type Credentials;
    type Error;
    type Id;
    type Account: Account;

    fn unlock(
        &mut self,
        account: Self::Id,
        cred: impl Into<Self::Credentials>,
    ) -> impl core::future::Future<Output = Result<Self::Account, Self::Error>>;
}

mod utils {
    const MAX_PATH_LEN: usize = 16;
    use arrayvec::ArrayString;

    use crate::{account::Account, any, any::AnySignature, Derive, Network, Pair, Public};

    /// The root account is a container of the key pairs stored in the vault and cannot be
    /// used to sign messages directly, we always derive new key pairs from it to create
    /// and use accounts with the wallet.
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
    }

    impl Derive for &RootAccount {
        type Pair = any::Pair;

        fn derive(&self, path: &str) -> Self::Pair {
            log::info!("derive: {}", path);
            match path.get(..2) {
                Some("//") => self.sub.derive(path).into(),
                _ => self.sub.derive("//default").into(),
            }
        }
    }

    /// Account is an abstraction around public/private key pairs that are more convenient to use and
    /// can hold extra metadata. Accounts are constructed by the wallet and are used to sign messages.
    pub struct AccountSigner {
        pair: Option<any::Pair>,
        network: Network,
        path: ArrayString<MAX_PATH_LEN>,
        name: ArrayString<{ MAX_PATH_LEN - 2 }>,
    }

    impl core::fmt::Debug for AccountSigner {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("AccountSigner")
                .field("network", &self.network)
                .field("name", &self.name.as_str())
                .field("locked", &self.pair.is_none())
                .finish()
        }
    }

    impl Drop for AccountSigner {
        fn drop(&mut self) {
            self.pair = None;
        }
    }

    impl Account for AccountSigner {
        fn public(&self) -> impl Public {
            self.pair
                .as_ref()
                .map(|p| p.public())
                .expect("account unlocked")
        }
    }

    impl AccountSigner {
        pub(crate) fn new<'a>(name: impl Into<Option<&'a str>>) -> Self {
            let n = name.into().unwrap_or("default");
            let mut path = ArrayString::from("//").unwrap();
            path.push_str(n);
            AccountSigner {
                pair: None,
                network: Network::default(),
                name: ArrayString::from(n).expect("short name"),
                path,
            }
        }

        pub fn switch_network(mut self, net: impl Into<Network>) -> Self {
            self.network = net.into();
            self
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn network(&self) -> &Network {
            &self.network
        }

        pub fn is_locked(&self) -> bool {
            self.pair.is_none()
        }

        pub(crate) fn unlock(mut self, root: &RootAccount) -> Self {
            if self.is_locked() {
                log::info!("unlock: {}", self.path);
                self.pair = Some(root.derive(&self.path));
            }
            self
        }
    }

    impl crate::Signer for AccountSigner {
        type Signature = AnySignature;

        async fn sign_msg(&self, msg: impl AsRef<[u8]>) -> Result<Self::Signature, crate::SigningError> {
            self.pair
                .as_ref()
                .ok_or(crate::SigningError::Locked)?
                .sign_msg(msg)
                .await
        }

        async fn verify(&self, msg: impl AsRef<[u8]>, sig: impl AsRef<[u8]>) -> bool {
            match self.pair.as_ref() {
                Some(p) => p.verify(msg, sig).await,
                None => false,
            }
        }
    }

    #[cfg(feature = "serde")]
    impl serde::Serialize for AccountSigner {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            use serde::ser::SerializeStruct;

            let mut state = serializer.serialize_struct("Account", 1)?;
            state.serialize_field("network", &self.network)?;
            state.serialize_field("path", self.path.as_str())?;
            state.serialize_field("name", self.name.as_str())?;
            state.end()
        }
    }

    impl core::fmt::Display for AccountSigner {
        fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            for byte in self.public().as_ref() {
                write!(f, "{:02x}", byte)?;
            }
            Ok(())
        }
    }
}
