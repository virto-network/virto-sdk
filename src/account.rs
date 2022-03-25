use crate::{CryptoType, Network, Pair};
use core::mem;

const ROOT_ACCOUNT: &str = "ROOT";

/// Account is an abstration around public/private key pairs that are more convenient to use and
/// can hold extra metadata. Accounts can only be constructed by the wallet and can be either a
/// root account or a sub-account derived from a root account.
#[derive(Debug)]
pub enum Account<'a, P> {
    Root {
        pair: P,
        network: Network,
        pending_sign: Vec<Vec<u8>>,
    },
    Sub {
        path: &'a str,
        name: &'a str,
        network: Network,
        pending_sign: Vec<Vec<u8>>,
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
            pending_sign: Vec::new(),
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

    /// Save data to be signed later
    pub fn add_to_pending(&mut self, message: &[u8]) {
        self.pending_sign_mut().push(message.into());
    }

    /// Sign messages from the queue returning them and their signatures
    pub fn sign_pending(&mut self) -> Vec<(Vec<u8>, P::Signature)> {
        let v = mem::take(self.pending_sign_mut());
        v.into_iter()
            .map(|msg| {
                let s = self.sign(&msg);
                (msg, s)
            })
            .collect()
    }

    // Return an iterator over the messages pending for signature in this account
    pub fn get_pending(&self) -> impl Iterator<Item = &[u8]> {
        self.pending_sign().iter().map(|i| i.as_ref())
    }

    fn pending_sign_mut(&mut self) -> &mut Vec<Vec<u8>> {
        match self {
            Self::Root { pending_sign, .. } | Self::Sub { pending_sign, .. } => pending_sign,
        }
    }

    fn pending_sign(&self) -> &Vec<Vec<u8>> {
        match self {
            Self::Root { pending_sign, .. } | Self::Sub { pending_sign, .. } => pending_sign,
        }
    }
}

impl<P: Pair> CryptoType for Account<'_, P> {
    type Pair = P;
}
