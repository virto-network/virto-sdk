use crate::{
    any::{self, AnySignature},
    Derive, Error, Network, Pair, Public, RootAccount,
};
use arrayvec::ArrayString;
// use regex::Regex;
// use sp_core::crypto::DeriveJunction;

const NAME_MAX_LEN: usize = 16;

/// Account is an abstration around public/private key pairs that are more convenient to use and
/// can hold extra metadata. Accounts can only be constructed by the wallet and can be either a
/// root account or a sub-account derived from a root account.
#[derive(Debug)]
pub struct Account<'a> {
    root: &'a RootAccount,
    network: Network,
    name: ArrayString<NAME_MAX_LEN>,
}

impl<'a> Account<'a> {
    pub(crate) fn new(root: &'a RootAccount, name: impl Into<Option<&'a str>>) -> Self {
        Account {
            root,
            network: Network::default(),
            //pending_sign: Vec::new(),
            name: ArrayString::from(name.into().unwrap_or_else(|| "default")).expect("short name"),
        }
    }

    pub(crate) fn with_network(self, net: impl Into<Network>) -> Self {
        Account {
            network: net.into(),
            ..self
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn public(&self) -> impl Public {
        self.account().expect("derive account").public()
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    fn account(&self) -> crate::Result<any::Pair> {
        self.root
            .derive(&self.name)
            .ok_or_else(|| Error::DeriveError)
    }
}

impl<'a> crate::Signer for Account<'a> {
    type Signature = AnySignature;

    fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
        self.account().expect("derive").sign_msg(msg)
    }
}

#[cfg(feature = "serde")]
impl<'a> serde::Serialize for Account<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("Account", 1)?;
        match self {
            Self::Root { network, .. } => {
                state.serialize_field("account_type", "root")?;
                //state.serialize_field("seed", pair)?;
                state.serialize_field("network", network)?;
                state.end()
            }
            Self::Sub {
                name,
                path,
                network,
                ..
            } => {
                state.serialize_field("account_type", "sub")?;
                state.serialize_field("name", name)?;
                state.serialize_field("derivation_path", path)?;
                state.serialize_field("network", network)?;
                state.end()
            }
        }
    }
}

impl<'a> core::fmt::Display for Account<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for byte in self.public().as_ref() {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}
