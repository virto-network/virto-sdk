use crate::{async_trait, CryptoType, Error, Pair, Result, Vault};

/// A vault that holds secrets in memory
pub struct SimpleVault<T: Pair> {
    seed: T::Seed,
}

impl<T: Pair> CryptoType for SimpleVault<T> {
    type Pair = T;
}

impl<T: Pair> SimpleVault<T> {
    /// A vault with a random seed, once dropped the the vault can't be restored
    /// ```
    /// # use libwallet::{SimpleVault, Vault, Result, sr25519};
    /// # #[async_std::main] async fn main() -> Result<()> {
    /// let vault = SimpleVault::<sr25519::Pair>::new();
    /// assert!(vault.unlock("").await.is_ok());
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        SimpleVault {
            seed: <Self as CryptoType>::Pair::generate().1,
        }
    }

    /// A vault with a password and random seed
    #[cfg(feature = "std")]
    pub fn new_with_password(pwd: &str) -> Self {
        SimpleVault {
            seed: <Self as CryptoType>::Pair::generate_with_phrase(Some(pwd)).2,
        }
    }

    // Provide your own seed
    pub fn new_with_seed(seed: T::Seed) -> Self {
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
impl<T: Pair> core::str::FromStr for SimpleVault<T> {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let seed = <Self as CryptoType>::Pair::from_string_with_seed(s, None)
            .map_err(|_| Error::InvalidPhrase)?
            .1
            .ok_or(Error::InvalidPhrase)?;
        Ok(SimpleVault { seed })
    }
}

#[async_trait(?Send)]
impl<T: Pair> Vault for SimpleVault<T> {
    async fn unlock(&self, _pwd: &str) -> Result<T> {
        let foo = <Self as CryptoType>::Pair::from_seed(&self.seed);
        Ok(foo)
    }
}
