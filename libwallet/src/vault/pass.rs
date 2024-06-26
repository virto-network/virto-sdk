use core::marker::PhantomData;

use arrayvec::ArrayVec;
use mnemonic::Language;
use prs_lib::{
    crypto::{self, IsContext, Proto},
    store::{FindSecret, Store},
    Plaintext,
};

use crate::{
    account,
    util::{seed_from_entropy, Pin},
    vault::utils::{AccountSigner, RootAccount},
    Vault,
};

/// A vault that stores secrets in a `pass` compatible repository
pub struct Pass<Id> {
    store: Store,
    root: Option<RootAccount>,
    auto_generate: Option<Language>,
    _phantom_data: PhantomData<Id>,
}

const DEFAULT_DIR: &str = "libwallet_accounts/";

impl<Id> Pass<Id> {
    /// Create a new `Pass` vault in the given location.
    /// The optional `lang` instructs the vault to generarte a backup phrase
    /// in the given language in case one does not exist.
    pub fn new<P: AsRef<str>>(store_path: P, lang: impl Into<Option<Language>>) -> Self {
        let store = Store::open(store_path).unwrap();

        Pass {
            store,
            root: None,
            auto_generate: lang.into(),
            _phantom_data: Default::default(),
        }
    }

    fn get_key(&self, credentials: &PassCreds) -> Result<RootAccount, Error> {
        let mut secret_path = String::from(DEFAULT_DIR);
        secret_path.push_str(&credentials.account);

        let secret = match self.store.find(Some(secret_path)) {
            FindSecret::Exact(secret) => Some(secret),
            FindSecret::Many(secrets) => secrets.first().cloned(),
        };

        let secret = secret.ok_or(Error::NotFound)?;
        let plaintext = crypto::context(Proto::Gpg)
            .map_err(|_e| Error::Decrypt)?
            .decrypt_file(&secret.path)
            .map_err(|_e| Error::Decrypt)?;

        let phrase = plaintext.unsecure_to_str().map_err(|_e| Error::Plaintext)?;
        let phrase = phrase
            .parse::<mnemonic::Mnemonic>()
            .map_err(|_e| Error::Plaintext)?;

        let seed = phrase.entropy();
        seed_from_entropy!(seed, credentials.pin.unwrap_or_default());
        Ok(RootAccount::from_bytes(seed))
    }

    #[cfg(all(feature = "rand", feature = "mnemonic"))]
    fn generate(&self, credentials: &PassCreds, lang: Language) -> Result<RootAccount, Error> {
        let map_encrypt_error = |e| {
            dbg!(e);
            Error::Encrypt
        };

        let phrase = crate::util::gen_phrase(&mut rand_core::OsRng, lang);

        let mut secret_path = String::from(DEFAULT_DIR);
        secret_path.push_str(&credentials.account);
        let secret_path = self
            .store
            .normalize_secret_path(secret_path, None, true)
            .map_err(map_encrypt_error)?;

        let plaintext = Plaintext::from(phrase.to_string());

        crypto::context(Proto::Gpg)
            .map_err(|_e| Error::Encrypt)?
            .encrypt_file(
                &self.store.recipients().map_err(map_encrypt_error)?,
                plaintext,
                &secret_path,
            )
            .map_err(map_encrypt_error)?;

        let seed = phrase.entropy();
        seed_from_entropy!(seed, credentials.pin.unwrap_or_default());
        Ok(RootAccount::from_bytes(seed))
    }
}

#[derive(Debug)]
pub enum Error {
    Store,
    NotFound,
    SecretPath,
    Encrypt,
    Decrypt,
    Plaintext,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::Store => write!(f, "Store load error"),
            Error::NotFound => write!(f, "Secret not found"),
            Error::SecretPath => write!(f, "Could not unwrap the secret path"),
            Error::Encrypt => write!(f, "Could not encrypt the secret"),
            Error::Decrypt => write!(f, "Could not decrypt the secret"),
            Error::Plaintext => write!(f, "Could not generate or unwrap the plaintext"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

#[derive(Clone)]
pub struct PassCreds {
    account: String,
    pin: Option<Pin>,
}

impl From<String> for PassCreds {
    fn from(account: String) -> Self {
        PassCreds {
            account,
            pin: Some(Pin::from("")),
        }
    }
}

impl<Id: AsRef<str>> Vault for Pass<Id> {
    type Id = Option<Id>;
    type Credentials = PassCreds;
    type Account = AccountSigner;
    type Error = Error;

    async fn unlock(
        &mut self,
        path: Self::Id,
        creds: impl Into<Self::Credentials>,
    ) -> Result<Self::Account, Self::Error> {
        let credentials = creds.into();

        self.get_key(&credentials)
            .or_else(|err| {
                self.auto_generate
                    .ok_or(err)
                    .and_then(|l| self.generate(&credentials, l))
            })
            .map(|r| {
                let acc = AccountSigner::new(path.as_ref().map(|x| x.as_ref())).unlock(&r);
                self.root = Some(r);
                acc
            })
    }
}
