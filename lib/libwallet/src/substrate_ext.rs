use crate::Network;

impl From<&str> for Network {
    fn from(s: &str) -> Self {
        match s {
            "polkadot" => Network::Substrate(0),
            "kusama" => Network::Substrate(2),
            "karura" => Network::Substrate(8),
            "substrate" | _ => Network::Substrate(42),
        }
    }
}
