use core::future::{ready, Ready};
use std::convert::TryInto;

use crate::{Pair, Result, Vault};

/// A vault that holds secrets in memory
pub struct SimpleVault<P: Pair> {
    seed: P::Seed,
}

impl<P, const S: usize> SimpleVault<P>
where
    P: Pair<Seed = [u8; S]>,
{
    /// A vault with a random seed, once dropped the the vault can't be restored
    /// ```
    /// # use libwallet::{SimpleVault, Vault, Result, sr25519};
    /// # #[async_std::main] async fn main() -> Result<()> {
    /// let mut vault = SimpleVault::<sr25519::Pair>::new();
    /// assert!(vault.unlock(()).await.is_ok());
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        let (_, seed) = P::generate();
        SimpleVault { seed }
    }

    // Provide your own seed
    pub fn from_phrase<T: AsRef<str>>(phrase: T) -> Self {
        let seed = phrase
            .as_ref()
            .parse::<mnemonic::Mnemonic>()
            .expect("mnemonic")
            .entropy()
            .try_into()
            .unwrap();
        SimpleVault { seed }
    }
}

impl<P: Pair> Vault for SimpleVault<P> {
    type Pair = P;
    type PairFut = Ready<Result<Self::Pair>>;

    fn unlock<C>(&mut self, _: C) -> Self::PairFut {
        let (pair, _) = Self::Pair::from_bytes(self.seed.as_ref());
        ready(Ok(pair))
    }
}
