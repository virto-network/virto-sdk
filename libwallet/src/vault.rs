//! Collection of supported Vault backends
#[cfg(feature = "vault_os")]
mod os;

#[cfg(feature = "vault_pjs")]
pub mod pjs;

#[cfg(feature = "vault_pjs")]
pub use pjs::*;

#[cfg(feature = "vault_pass")]
mod pass;

#[cfg(feature = "vault_pass")]
pub use pass::*;

mod simple;
pub use simple::*;
use crate::account::Account;

/// Abstraction for storage of private keys that are protected by some credentials.
pub trait Vault {
    type Credentials;
    type Error;
    type Id;
    type Account: Account;

    async fn unlock(
        &mut self,
        account: Self::Id,
        cred: impl Into<Self::Credentials>,
    ) -> Result<Self::Account, Self::Error>;
}

mod utils {
    const MAX_PATH_LEN: usize = 16;
    use arrayvec::ArrayString;

    use crate::{account::Account, any, any::AnySignature, Derive, Network, Pair, Public};

    /// The root account is a container of the key pairs stored in the vault and cannot be
    /// used to sign messages directly, we always derive new key pairs from it to create
    /// and use accounts with the wallet.
    #[derive(Debug)]
    pub struct RootAccount {
        #[cfg(feature = "substrate")]
        sub: crate::key_pair::sr25519::Pair,
    }

    impl RootAccount {
        pub fn from_bytes(seed: &[u8]) -> Self {
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
            if path.is_empty() {
                return self.sub.derive("").into();
            }

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

    /// Account is an abstration around public/private key pairs that are more convenient to use and
    /// can hold extra metadata. Accounts are constructed by the wallet and are used to sign messages.
    #[derive(Debug)]
    pub struct AccountSigner {
        pair: Option<any::Pair>,
        network: Network,
        path: ArrayString<MAX_PATH_LEN>,
        name: ArrayString<{ MAX_PATH_LEN - 2 }>,
    }

    impl Account for AccountSigner {
        fn public(&self) -> impl Public {
            self.pair.as_ref().expect("account unlocked").public()
        }
    }

    impl AccountSigner {
        pub(crate) fn new<'a>(name: impl Into<Option<&'a str>>) -> Self {
            let n = name.into().unwrap_or_else(|| "");
            let mut path = ArrayString::new();

            if n == "default" {
                path.push_str(n);
            }

            if n != "default" && !n.is_empty() {
                path.push_str("//");
                path.push_str(n);
            }

            AccountSigner {
                pair: None,
                network: Network::default(),
                name: ArrayString::from(n).expect("short name"),
                path,
            }
        }

        pub fn switch_network(self, net: impl Into<Network>) -> Self {
            AccountSigner {
                network: net.into(),
                ..self
            }
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
                self.pair = Some(root.derive(&self.path));
            }
            self
        }
    }

    impl crate::Signer for AccountSigner {
        type Signature = AnySignature;

        async fn sign_msg(&self, msg: impl AsRef<[u8]>) -> Result<Self::Signature, ()> {
            Ok(self
                .pair
                .as_ref()
                .expect("account unlocked")
                .sign_msg(msg)
                .await?)
        }

        async fn verify(&self, msg: impl AsRef<[u8]>, sig: impl AsRef<[u8]>) -> bool {
            self.pair
                .as_ref()
                .expect("account unlocked")
                .verify(msg, sig)
                .await
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
