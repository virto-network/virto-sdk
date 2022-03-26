use core::convert::TryInto;

use crate::{Account, Network, Vault, Wallet};
use sp_core::Pair;

pub use sp_core::crypto::Ss58AddressFormat;

trait SubstrateExt {}

impl<V: Vault> SubstrateExt for Wallet<V> {}

impl<T> SubstrateExt for Account<T> {}

#[cfg(feature = "std")]
impl<P> core::fmt::Display for Account<P>
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

impl From<&Network> for Ss58AddressFormat {
    fn from(n: &Network) -> Self {
        match n {
            Network::Substrate(prefix) => (*prefix).try_into().expect("valid substrate prefix"),
        }
    }
}
