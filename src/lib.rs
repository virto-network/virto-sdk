//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.
use async_trait::async_trait;
pub use bip39::{Language, Mnemonic, MnemonicType, Seed};
use sp_core::{crypto::CryptoType, sr25519, Pair};

#[cfg(feature = "chain")]
pub mod chain;

#[async_trait]
pub trait Valut {
    async fn store(&self, secret: &[u8]) -> Result<()>;
    async fn unlock(&self, password: &str) -> Result<Seed>;
}

type Result<T> = std::result::Result<T, Error>;

/// Wallet is the main interface to manage and interact with accounts.  
#[derive(Default)]
pub struct Wallet<'a> {
    seed: Option<Seed>,
    vault: Option<&'a dyn Valut>,
}

impl<'a> Wallet<'a> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_vault(self, vault: &'a dyn Valut) -> Self {
        Wallet {
            vault: Some(vault),
            ..self
        }
    }

    /// Generate a new wallet with a 24 word english mnemonic seed
    pub fn generate(password: &str) -> (Self, String) {
        let phrase = mnemonic(Language::English);
        (Wallet::import(&phrase, password).unwrap(), phrase)
    }

    /// Import a wallet from its mnemonic seed
    /// ```
    /// # use libwallet::{Language, Wallet, mnemonic};
    /// let phrase = mnemonic(Language::English);
    /// let mut wallet = Wallet::import(&phrase, "").unwrap();
    /// # assert_eq!(wallet.is_locked(), false);
    /// ```
    pub fn import(seed_phrase: &str, password: &str) -> Result<Self> {
        let mnemonic = Mnemonic::from_phrase(seed_phrase, Language::English)
            .map_err(|_| Error::InvalidPhrase)?;
        let seed = Some(Seed::new(&mnemonic, password));
        Ok(Wallet {
            seed,
            ..Default::default()
        })
    }

    pub fn default_account(&self) -> Result<<Self as CryptoType>::Pair> {
        let seed = self.seed.as_ref().ok_or(Error::Locked)?.as_bytes();
        let default =
            <Self as CryptoType>::Pair::from_seed_slice(&seed[..32]).expect("seed is valid");
        Ok(default)
    }

    /// A locked wallet can use a vault to retrive its secret seed.
    /// ```
    /// # use libwallet::{Wallet, Error, Seed, Mnemonic, MnemonicType, Language};
    /// # use libwallet::Valut;
    /// # struct Dummy;
    /// # #[async_trait::async_trait] impl Valut for Dummy {
    /// #   async fn store(&self, _: &[u8]) -> Result<(), Error> { todo!() }
    /// #   async fn unlock(&self, pwd: &str) -> Result<Seed, Error> {
    /// #       Ok(Seed::new(&Mnemonic::new(MnemonicType::Words12, Language::English), pwd))
    /// #   }
    /// # }
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let dummy_vault = Dummy{};
    /// let mut wallet = Wallet::new().with_vault(&dummy_vault);
    /// if wallet.is_locked() {
    ///     wallet.unlock("some password").await?;
    /// }
    /// # assert_eq!(wallet.is_locked(), false);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock(&mut self, password: &str) -> Result<()> {
        if !self.is_locked() {
            return Ok(());
        }
        let seed = self.vault.ok_or(Error::NoVault)?.unlock(password).await?;
        self.seed = Some(seed);
        Ok(())
    }

    pub fn is_locked(&self) -> bool {
        self.seed.is_none()
    }

    /// Sign a message with the default account and return the 512bit signature.
    /// ```
    /// # use libwallet::Wallet;
    /// let (wallet, _) = Wallet::generate("");
    /// let signature = wallet.sign(&[0x01, 0x02, 0x03]);
    /// assert!(signature.is_ok());
    /// ```
    pub fn sign(&self, msg: &[u8]) -> Result<[u8; 64]> {
        let signature = self.default_account()?.sign(msg);
        Ok(signature.into())
    }
}

impl<'a> CryptoType for Wallet<'a> {
    type Pair = sr25519::Pair;
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
