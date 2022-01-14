use crate::{CryptoType, Network, Pair};

const ROOT_ACCOUNT: &str = "ROOT";

/// Account is an abstration around public/private key pairs that are more convenient to use and
/// can hold extra metadata. Accounts can only be constructed by the wallet and can be either a
/// root account or a sub-account derived from a root account.
#[derive(Debug)]
pub enum Account<'a, P> {
    Root {
        pair: P,
        network: Network,
    },
    Sub {
        path: &'a str,
        name: &'a str,
        network: Network,
    },
}

impl<'a, P> Account<'a, P>
where
    P: Pair,
{
    pub(crate) fn from_pair(pair: P) -> Self {
        Account::Root {
            pair,
            network: Network::default(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Root { .. } => ROOT_ACCOUNT,
            Self::Sub { name, .. } => name,
        }
    }

    pub fn public(&self) -> P::Public {
        self.pair().public()
    }

    pub fn sign(&self, message: &[u8]) -> P::Signature {
        self.pair().sign(message)
    }

    pub fn network(&self) -> &Network {
        match self {
            Self::Root { network, .. } | Self::Sub { network, .. } => network,
        }
    }

    pub fn switch_network(mut self, network: Network) -> Self {
        *self.network_mut() = network;
        self
    }

    fn network_mut(&mut self) -> &mut Network {
        match self {
            Self::Root { network, .. } | Self::Sub { network, .. } => network,
        }
    }

    fn pair(&self) -> &P {
        match self {
            Self::Root { pair, .. } => pair,
            Self::Sub { .. } => todo!(),
        }
    }
}

impl<P: Pair> CryptoType for Account<'_, P> {
    type Pair = P;
}

#[cfg(feature = "std")]
impl<P> core::fmt::Display for Account<'_, P>
where
    P: Pair,
    P::Public: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let account = self.public();
        let net = self.network();
        write!(
            f,
            "{}",
            sp_core::crypto::Ss58Codec::to_ss58check_with_version(&account, net.into())
        )
    }
}
