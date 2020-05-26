//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.
use async_trait::async_trait;
pub use bip39::{Language, Mnemonic, MnemonicType};
use sp_core::{crypto::CryptoType, sr25519, Pair};
use std::convert::TryFrom;
use std::fmt;
use zeroize::Zeroize;

#[cfg(feature = "chain")]
pub mod chain;

#[async_trait]
pub trait Vault: Send {
    async fn unlock(&self, password: String) -> Result<Seed>;
}

type Result<T> = std::result::Result<T, Error>;

/// Wallet is the main interface to manage and interact with accounts.  
#[derive(Default)]
pub struct Wallet {
    seed: Option<Seed>,
    vault: Option<Box<dyn Vault>>,
}

impl Wallet {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_vault(self, vault: Box<dyn Vault>) -> Self {
        Wallet {
            vault: Some(vault),
            ..self
        }
    }

    /// Generate a new wallet with a 24 word english mnemonic seed
    pub fn generate(password: String) -> (Self, String) {
        let phrase = mnemonic(Language::English);
        (Wallet::import(phrase.clone(), password).unwrap(), phrase)
    }

    /// Import a wallet from its mnemonic seed
    /// ```
    /// # use libwallet::{Language, Wallet, mnemonic};
    /// let phrase = mnemonic(Language::English);
    /// let mut wallet = Wallet::import(phrase, "foo".to_owned()).unwrap();
    /// # assert_eq!(wallet.is_locked(), false);
    /// ```
    pub fn import(seed_phrase: String, password: String) -> Result<Self> {
        let mnemonic = Mnemonic::from_phrase(&seed_phrase, Language::English)
            .map_err(|_| Error::InvalidPhrase)?;
        let seed = bip39::Seed::new(&mnemonic, &password).into();
        Ok(Wallet {
            seed: Some(seed),
            ..Default::default()
        })
    }

    pub fn default_account(&self) -> Result<<Self as CryptoType>::Pair> {
        let seed = self.seed.as_ref().ok_or(Error::Locked)?.as_ref();
        let default =
            <Self as CryptoType>::Pair::from_seed_slice(&seed[..32]).expect("seed is valid");
        Ok(default)
    }

    /// A locked wallet can use a vault to retrive its secret seed.
    /// ```
    /// # use libwallet::{Wallet, Error, Seed, mnemonic, Language, Vault};
    /// # use std::convert::TryInto;
    /// # struct Dummy;
    /// # #[async_trait::async_trait(?Send)] impl Vault for Dummy {
    /// #   async fn unlock(&self, pwd: String) -> Result<Seed, Error> {
    /// #       (mnemonic(Language::English), pwd).try_into()
    /// #   }
    /// # }
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let dummy_vault = Box::new(Dummy{});
    /// let mut wallet = Wallet::new().with_vault(dummy_vault);
    /// if wallet.is_locked() {
    ///     wallet.unlock("some password".to_owned()).await?;
    /// }
    /// # assert_eq!(wallet.is_locked(), false);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock(&mut self, password: String) -> Result<()> {
        if !self.is_locked() {
            return Ok(());
        }
        let seed = self
            .vault
            .as_ref()
            .ok_or(Error::NoVault)?
            .unlock(password)
            .await?;
        self.seed = Some(seed).into();
        Ok(())
    }

    pub fn is_locked(&self) -> bool {
        self.seed.is_none()
    }

    /// Sign a message with the default account and return the 512bit signature.
    /// ```
    /// # use libwallet::Wallet;
    /// let (wallet, _) = Wallet::generate("foo".to_owned());
    /// let signature = wallet.sign(&[0x01, 0x02, 0x03]);
    /// assert!(signature.is_ok());
    /// ```
    pub fn sign(&self, msg: &[u8]) -> Result<[u8; 64]> {
        let signature = self.default_account()?.sign(msg);
        Ok(signature.into())
    }
}

impl CryptoType for Wallet {
    type Pair = sr25519::Pair;
}

#[derive(Zeroize)]
#[zeroize(drop)]
pub struct Seed([u8; 64]);

impl TryFrom<(String, String)> for Seed {
    type Error = Error;

    fn try_from((phrase, pwd): (String, String)) -> Result<Self> {
        let seed = bip39::Seed::new(
            &Mnemonic::from_phrase(&phrase, Language::English).map_err(|_| Error::InvalidPhrase)?,
            &pwd,
        );
        Ok(seed.into())
    }
}

impl From<bip39::Seed> for Seed {
    fn from(seed: bip39::Seed) -> Self {
        let mut s = [0; 64];
        s.copy_from_slice(&seed.as_bytes()[..64]);
        Self(s)
    }
}

impl AsRef<[u8]> for Seed {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl fmt::Debug for Seed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<secret>")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid mnemonic phrase")]
    InvalidPhrase,
    #[error("Invalid password")]
    InvalidPasword,
    #[error("Wallet is locked")]
    Locked,
    #[error("Can't unlock, no vault was configured")]
    NoVault,
}

/// Generate a 24 word mnemonic phrase with words in the specified language.
/// ```
/// # use libwallet::{mnemonic, Language};
/// let phrase = mnemonic(Language::English);
/// # let words = phrase.split_whitespace().count();
/// # assert_eq!(words, 24);
/// ```
pub fn mnemonic(lang: Language) -> String {
    Mnemonic::new(MnemonicType::Words24, lang)
        .phrase()
        .to_owned()
}
