use crate::{Account, Network, Vault, Wallet};

trait SubstrateExt {}

impl<V: Vault> SubstrateExt for Wallet<V> {}

impl<'a> SubstrateExt for Account<'a> {}

impl From<&str> for Network {
    fn from(s: &str) -> Self {
        // TODO use registry
        match s {
            "polkadot" => Network::Substrate(0),
            "kusama" => Network::Substrate(2),
            "karura" => Network::Substrate(8),
            "substrate" => Network::Substrate(42),
            _ => Network::default(),
        }
    }
}
