use crate::{async_trait, Box, Pair, Result, Vault};

/// A vault that holds secrets in memory
pub struct SimpleVault<P: Pair> {
    seed: P::Seed,
}

impl<P: Pair> SimpleVault<P> {
    /// A vault with a random seed, once dropped the the vault can't be restored
    /// ```
    /// # use libwallet::{SimpleVault, Vault, Result, sr25519};
    /// # #[async_std::main] async fn main() -> Result<()> {
    /// let vault = SimpleVault::<sr25519::Pair>::new();
    /// assert!(vault.unlock(()).await.is_ok());
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        SimpleVault {
            seed: P::generate().1,
        }
    }

    /// A vault with a password and random seed
    #[cfg(feature = "std")]
    pub fn new_with_password(pwd: &str) -> Self {
        SimpleVault {
            seed: P::generate_with_phrase(Some(pwd)).2,
        }
    }

    // Provide your own seed
    pub fn new_with_seed(seed: P::Seed) -> Self {
        SimpleVault { seed }
    }
}

#[cfg(feature = "std")]
impl<T: Pair> From<&str> for SimpleVault<T> {
    fn from(s: &str) -> Self {
        s.parse().expect("valid secret string")
    }
}

#[cfg(feature = "std")]
impl<P: Pair> core::str::FromStr for SimpleVault<P> {
    type Err = crate::Error;
    fn from_str(s: &str) -> Result<Self> {
        let seed = P::from_string_with_seed(s, None)
            .map_err(|_| Self::Err::InvalidPhrase)?
            .1
            .ok_or(Self::Err::InvalidPhrase)?;
        Ok(SimpleVault { seed })
    }
}

#[async_trait(?Send)]
impl<P: Pair> Vault for SimpleVault<P> {
    type Pair = P;

    async fn unlock(&self, _: ()) -> Result<P> {
        let foo = Self::Pair::from_seed(&self.seed);
        Ok(foo)
    }
}
