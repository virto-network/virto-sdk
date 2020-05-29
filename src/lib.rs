//! With `libwallet` you can build crypto currency wallets that
//! manage private keys of different kinds saved in a secure storage.
use async_trait::async_trait;
pub use bip39::{Language, Mnemonic, MnemonicType};
use sp_core::{sr25519, Pair};
use std::convert::TryFrom;
use std::fmt::{self, Display};
use std::ops::Deref;
use zeroize::Zeroize;

#[cfg(feature = "chain")]
pub mod chain;

#[async_trait]
pub trait Vault: fmt::Debug + Send {
    async fn unlock(&self, password: String) -> Result<Seed>;
}

#[async_trait]
impl Vault for () {
    async fn unlock(&self, _: String) -> Result<Seed> {
        Err(Error::NoVault)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Wallet is the main interface to manage and interact with accounts.  
#[derive(Debug)]
pub struct Wallet<V: Vault> {
    seed: Option<Seed>,
    vault: Option<V>,
}

impl Wallet<()> {
    pub fn new() -> Self {
        Default::default()
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

    /// Generate a new wallet with a 24 word english mnemonic seed
    pub fn generate(password: String) -> (Self, String) {
        let phrase = mnemonic(Language::English);
        (Wallet::import(phrase.clone(), password).unwrap(), phrase)
    }
}

impl<V: Vault> Wallet<V> {
    pub fn with_vault<V2: Vault>(self, vault: V2) -> Wallet<V2> {
        Wallet {
            vault: Some(vault),
            seed: self.seed,
        }
    }

    pub fn root_account(&self) -> Result<Account<sr25519::Pair>> {
        let seed = self.seed.as_ref().ok_or(Error::Locked)?.as_ref();
        let root = Account::from_seed(seed);
        Ok(root)
    }

    /// A locked wallet can use a vault to retrive its secret seed.
    /// ```
    /// # use libwallet::{Wallet, Error, Seed, mnemonic, Language, Vault};
    /// # use std::convert::TryInto;
    /// # #[derive(Debug, Default)] struct Dummy;
    /// # #[async_trait::async_trait] impl Vault for Dummy {
    /// #   async fn unlock(&self, pwd: String) -> Result<Seed, Error> {
    /// #       (mnemonic(Language::English), pwd).try_into()
    /// #   }
    /// # }
    /// # #[async_std::main] async fn main() -> Result<(), Error> {
    /// # let dummy_vault = Dummy{};
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
        let signature = self.root_account()?.sign(msg);
        Ok(signature.into())
    }
}

impl<V: Vault> Default for Wallet<V> {
    fn default() -> Self {
        Wallet {
            seed: None,
            vault: None,
        }
    }
}

pub struct Account<P>
where
    P: Pair,
    P::Public: Display,
{
    pair: P,
}

impl<P> Account<P>
where
    P: Pair,
    P::Public: Display,
{
    pub fn from_seed(seed: &[u8]) -> Self {
        let pair = P::from_seed_slice(&seed[..32]).expect("seed is valid");
        Account { pair }
    }

    pub fn id(&self) -> String {
        self.pair.public().to_string()
    }
}

impl<P> Deref for Account<P>
where
    P: Pair,
    P::Public: Display,
{
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.pair
    }
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
