use crate::{
    any::{self, AnySignature},
    Derive, Network, Pair, Public, RootAccount,
};
use arrayvec::ArrayString;
// use regex::Regex;
// use sp_core::crypto::DeriveJunction;

const MAX_PATH_LEN: usize = 16;

/// Account is an abstration around public/private key pairs that are more convenient to use and
/// can hold extra metadata. Accounts are constructed by the wallet and are used to sign messages.
#[derive(Debug)]
pub struct Account {
    pair: Option<any::Pair>,
    network: Network,
    path: ArrayString<MAX_PATH_LEN>,
    name: ArrayString<{ MAX_PATH_LEN - 2 }>,
}

impl Account {
    pub(crate) fn new<'a>(name: impl Into<Option<&'a str>>) -> Self {
        let n = name.into().unwrap_or_else(|| "default");
        let mut path = ArrayString::from("//").unwrap();
        path.push_str(&n);
        Account {
            pair: None,
            network: Network::default(),
            name: ArrayString::from(&n).expect("short name"),
            path,
        }
    }

    pub fn switch_network(self, net: impl Into<Network>) -> Self {
        Account {
            network: net.into(),
            ..self
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn public(&self) -> impl Public {
        self.pair.as_ref().expect("account unlocked").public()
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn is_locked(&self) -> bool {
        self.pair.is_none()
    }

    pub(crate) fn unlock(&mut self, root: &RootAccount) -> &Self {
        if self.is_locked() {
            self.pair = Some(root.derive(&self.path));
        }
        self
    }
}

impl crate::Signer for Account {
    type Signature = AnySignature;

    fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
        self.pair.as_ref().expect("account unlocked").sign_msg(msg)
    }

    fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool {
        self.pair
            .as_ref()
            .expect("account unlocked")
            .verify(msg, sig)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Account {
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

impl core::fmt::Display for Account {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for byte in self.public().as_ref() {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}
