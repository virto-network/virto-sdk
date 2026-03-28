use core::marker::PhantomData;

use crate::{
    mnemonic::{Language, Mnemonic},
    vault::{
        utils::{DerivedSigner, RootAccount},
        Vault,
    },
};
use keyring;

const SERVICE: &str = "libwallet_account";

/// A vault that stores keys in the default OS secure store
pub struct OSKeyring<S> {
    entry: keyring::Entry,
    auto_generate: Option<Language>,
    _phantom: PhantomData<S>,
}

impl<S> OSKeyring<S> {
    pub fn new(uname: &str, lang: impl Into<Option<Language>>) -> Self {
        OSKeyring {
            entry: keyring::Entry::new(SERVICE, uname),
            auto_generate: lang.into(),
            _phantom: PhantomData::default(),
        }
    }

    pub fn update(&self, phrase: &str) -> Result<(), Error> {
        self.entry.set_password(phrase).map_err(|_| Error::Keyring)
    }

    pub(crate) fn get(&self) -> Result<zeroize::Zeroizing<String>, Error> {
        self.entry
            .get_password()
            .map(zeroize::Zeroizing::new)
            .map_err(|_| Error::Keyring)
    }

    fn get_signer(&self, path: Option<&str>) -> Result<DerivedSigner, Error> {
        let phrase = self
            .get()?
            .parse::<Mnemonic>()
            .map_err(|_| Error::BadPhrase)?;

        let seed = crate::substrate_seed(phrase.entropy(), "");
        let root = RootAccount::from_bytes(&*seed).ok_or(Error::BadPhrase)?;
        let path = path.unwrap_or("//default");
        let pair = root.derive(path);
        Ok(DerivedSigner::new(pair, path))
    }

    fn generate(&self, path: Option<&str>, lang: Language) -> Result<DerivedSigner, Error> {
        let phrase = crate::util::gen_phrase(&mut rand_core::OsRng, lang);

        let seed = crate::substrate_seed(phrase.entropy(), "");
        let root = RootAccount::from_bytes(&*seed).ok_or(Error::BadPhrase)?;

        self.entry
            .set_password(phrase.phrase())
            .map_err(|_| Error::Keyring)?;

        let path = path.unwrap_or("//default");
        let pair = root.derive(path);
        Ok(DerivedSigner::new(pair, path))
    }
}

#[derive(Debug)]
pub enum Error {
    Keyring,
    NotFound,
    BadPhrase,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::Keyring => write!(f, "OS Key storage error"),
            Error::NotFound => write!(f, "Key not found"),
            Error::BadPhrase => write!(f, "Mnemonic is invalid"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl<S: AsRef<str>> Vault for OSKeyring<S> {
    type Credentials = ();
    type Error = Error;
    type Id = Option<S>;
    type Signer = DerivedSigner;

    async fn unlock(
        &mut self,
        account: Self::Id,
        _cred: impl Into<Self::Credentials>,
    ) -> Result<Self::Signer, Self::Error> {
        let path = account.as_ref().map(|x| x.as_ref());
        self.get_signer(path)
            .or_else(|err| {
                self.auto_generate
                    .ok_or(err)
                    .and_then(|l| self.generate(path, l))
            })
    }
}
