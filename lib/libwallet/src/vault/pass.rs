use core::marker::PhantomData;

use mnemonic::Language;
use prs_lib::{
    crypto::{self, IsContext, Proto},
    store::{FindSecret, Store},
    Plaintext,
};

use crate::vault::{
    utils::{DerivedSigner, RootAccount},
    Vault,
};

/// A vault that stores secrets in a `pass` compatible repository
pub struct Pass<Id> {
    store: Store,
    auto_generate: Option<Language>,
    _phantom_data: PhantomData<Id>,
}

const DEFAULT_DIR: &str = "libwallet_accounts/";

impl<Id> Pass<Id> {
    pub fn new<P: AsRef<str>>(store_path: P, lang: impl Into<Option<Language>>) -> Self {
        let store = Store::open(store_path).unwrap();

        Pass {
            store,
            auto_generate: lang.into(),
            _phantom_data: Default::default(),
        }
    }

    fn get_signer(&self, account: &str) -> Result<DerivedSigner, Error> {
        let mut secret_path = String::from(DEFAULT_DIR);
        secret_path.push_str(account);

        let secret = match self.store.find(Some(secret_path)) {
            FindSecret::Exact(secret) => Some(secret),
            FindSecret::Many(secrets) if secrets.len() == 1 => secrets.into_iter().next(),
            FindSecret::Many(_) => return Err(Error::AmbiguousMatch),
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

        let seed = crate::substrate_seed(phrase.entropy(), "");
        let root = RootAccount::from_bytes(&*seed).ok_or(Error::Plaintext)?;
        let pair = root.derive(&format!("//{account}"));
        Ok(DerivedSigner::new(pair))
    }

    #[cfg(all(feature = "rand", feature = "mnemonic"))]
    fn generate(&self, account: &str, lang: Language) -> Result<DerivedSigner, Error> {
        let phrase = crate::util::gen_phrase(&mut rand_core::OsRng, lang);

        let mut secret_path = String::from(DEFAULT_DIR);
        secret_path.push_str(account);
        let secret_path = self
            .store
            .normalize_secret_path(secret_path, None, true)
            .map_err(|_| Error::Encrypt)?;

        let plaintext = Plaintext::from(phrase.to_string());

        crypto::context(Proto::Gpg)
            .map_err(|_| Error::Encrypt)?
            .encrypt_file(
                &self.store.recipients().map_err(|_| Error::Encrypt)?,
                plaintext,
                &secret_path,
            )
            .map_err(|_| Error::Encrypt)?;

        let seed = crate::substrate_seed(phrase.entropy(), "");
        let root = RootAccount::from_bytes(&*seed).ok_or(Error::Plaintext)?;
        let pair = root.derive(&format!("//{account}"));
        Ok(DerivedSigner::new(pair))
    }
}

#[derive(Debug)]
pub enum Error {
    Store,
    NotFound,
    AmbiguousMatch,
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
            Error::AmbiguousMatch => write!(f, "Multiple secrets match, specify a unique name"),
            Error::SecretPath => write!(f, "Could not unwrap the secret path"),
            Error::Encrypt => write!(f, "Could not encrypt the secret"),
            Error::Decrypt => write!(f, "Could not decrypt the secret"),
            Error::Plaintext => write!(f, "Could not generate or unwrap the plaintext"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl<Id: AsRef<str>> Vault for Pass<Id> {
    type Id = Option<Id>;
    type Credentials = ();
    type Signer = DerivedSigner;
    type Error = Error;

    async fn unlock(
        &mut self,
        path: Self::Id,
        _creds: impl Into<Self::Credentials>,
    ) -> Result<Self::Signer, Self::Error> {
        let account = path.as_ref().map(|x| x.as_ref()).unwrap_or("default");

        self.get_signer(account)
            .or_else(|err| {
                self.auto_generate
                    .ok_or(err)
                    .and_then(|l| self.generate(account, l))
            })
    }
}
