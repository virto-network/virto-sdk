use core::{fmt::{Display, Write}, str::FromStr};

use crate::{
    any::{self, AnySignature},
    Derive, Network, Pair, Public, RootAccount,
};

use arrayvec::ArrayString;
// use regex::Regex;
// use sp_core::crypto::DeriveJunction;

const MAX_PATH_LEN: usize = 16;
const MAX_NAME_LEN: usize = MAX_PATH_LEN - 2;

/// Account is an abstration around public/private key pairs that are more convenient to use and
/// can hold extra metadata. Accounts are constructed by the wallet and are used to sign messages.
#[derive(Debug, Clone)]
pub struct Account {
    pair: Option<any::Pair>,
    network: Network,
    path: ArrayString<MAX_PATH_LEN>,
    name: ArrayString<MAX_NAME_LEN>,
}

pub enum AccountPath {
    Root,
    Path(ArrayString<MAX_PATH_LEN>),
    Default,
}



impl Display for AccountPath {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AccountPath::Root => write!(f, "root"),
            AccountPath::Path(s) => write!(f, "{}", s),
            AccountPath::Default => write!(f, "//default"),
        }
    }
}

impl Account {
    pub(crate) fn new(account_path: AccountPath) -> Self {
        let mut path = ArrayString::new();
        

        match account_path {
            AccountPath::Root => {}
            AccountPath::Path(s) => path = s,
            AccountPath::Default => path.push_str("//default"),
        }
        
        let mut name: ArrayString<MAX_NAME_LEN> = ArrayString::new();
        write!(&mut name, "{}", account_path);
        
        Account {
            pair: None,
            network: Network::default(),
            name,
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
