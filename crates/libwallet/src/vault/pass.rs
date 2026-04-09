use core::marker::PhantomData;

use mnemonic::Language;
use prs_lib::{
    crypto::{self, IsContext, Proto},
    store::{FindSecret, Store},
    Plaintext,
};

/// A key store backed by a `pass`-compatible GPG repository.
/// Wrap with `Substrate<Pass<..>>` to produce substrate-compatible signers.
pub struct Pass<Id> {
    store: Store,
    entropy: Option<zeroize::Zeroizing<Vec<u8>>>,
    auto_generate: Option<Language>,
    account: Option<arrayvec::ArrayString<32>>,
    _phantom_data: PhantomData<Id>,
}

const DEFAULT_DIR: &str = "libwallet_accounts/";

impl<Id> Pass<Id> {
    pub fn new<P: AsRef<str>>(store_path: P, lang: impl Into<Option<Language>>) -> Self {
        let store = Store::open(store_path).unwrap();

        Pass {
            store,
            entropy: None,
            auto_generate: lang.into(),
            account: None,
            _phantom_data: Default::default(),
        }
    }

    pub fn account(mut self, name: &str) -> Self {
        let mut buf = arrayvec::ArrayString::new();
        let len = name.len().min(32);
        let _ = buf.try_push_str(&name[..len]);
        self.account = Some(buf);
        self
    }

    fn load_entropy(&mut self) -> Result<(), Error> {
        let account = self.account.as_deref().unwrap_or("default");

        let phrase = self.get_phrase(account)
            .or_else(|err| {
                self.auto_generate
                    .ok_or(err)
                    .and_then(|l| self.generate_phrase(account, l))
            })?;

        let mnemonic = phrase
            .parse::<mnemonic::Mnemonic>()
            .map_err(|_| Error::Plaintext)?;

        self.entropy = Some(zeroize::Zeroizing::new(mnemonic.entropy().to_vec()));
        Ok(())
    }

    fn get_phrase(&self, account: &str) -> Result<String, Error> {
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

        plaintext.unsecure_to_str()
            .map(|s| s.to_string())
            .map_err(|_e| Error::Plaintext)
    }

    #[cfg(all(feature = "rand", feature = "mnemonic"))]
    fn generate_phrase(&self, account: &str, lang: Language) -> Result<String, Error> {
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

        Ok(phrase.phrase().to_string())
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

impl<Id> crate::chain::KeyStore for Pass<Id> {
    type Error = Error;
    fn unlock(&mut self) -> Result<&[u8], Self::Error> {
        if self.entropy.is_none() {
            self.load_entropy()?;
        }
        self.entropy.as_deref().ok_or(Error::NotFound)
    }
}
