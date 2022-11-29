use crate::{
    mnemonic::{Language, Mnemonic},
    util::Pin,
    RootAccount, Vault,
};
use core::future::Ready;
use keyring;

const SERVICE: &str = "libwallet_account";

/// A vault that stores keys in the default OS secure store
pub struct OSKeyring {
    entry: keyring::Entry,
    root: Option<RootAccount>,
    auto_generate: Option<Language>,
}

impl OSKeyring {
    /// Create a new OSKeyring vault for the given user.
    /// The optional `lang` instructs the vault to generarte a backup phrase
    /// in the given language in case one does not exist.
    pub fn new(uname: &str, lang: impl Into<Option<Language>>) -> Self {
        OSKeyring {
            entry: keyring::Entry::new(SERVICE, &uname),
            root: None,
            auto_generate: lang.into(),
        }
    }

    /// Relace the stored backap phrase with a new one.
    pub fn update(&self, phrase: &str) -> Result<(), ()> {
        self.entry.set_password(phrase).map_err(|_| ())
    }

    /// Returned the stored phrase from the OS secure storage
    pub fn get(&self) -> Result<String, Error> {
        self.entry
            .get_password()
            // .inspect_err(|e| {
            //     dbg!(e);
            // })
            .map_err(|_| Error::Keyring)
    }

    fn get_key(&self, pin: &Pin) -> Result<RootAccount, Error> {
        let phrase = self
            .get()?
            .parse::<Mnemonic>()
            .map_err(|_| Error::BadPhrase)?;

        let seed = pin.protect::<64>(phrase.entropy());
        Ok(RootAccount::from_bytes(&seed))
    }

    // Create new random seed and save it in the OS keyring.
    fn generate(&self, pin: &Pin, lang: Language) -> Result<RootAccount, Error> {
        let phrase = crate::util::gen_phrase(&mut rand_core::OsRng, lang);
        let root = RootAccount::from_bytes(&pin.protect::<64>(phrase.entropy()));
        self.entry
            .set_password(phrase.phrase())
            // .inspect_err(|e| {
            //     dbg!(e);
            // })
            .map_err(|e| {
                dbg!(e);
                Error::Keyring
            })?;
        Ok(root)
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

impl Vault for OSKeyring {
    type Credentials = Pin;
    type Error = Error;

    async fn unlock(&mut self, pin: impl Into<Self::Credentials>) -> Result<(), Self::Error> {
        let pin = pin.into();
        self.get_key(&pin)
            .or_else(|err| {
                self.auto_generate
                    .ok_or(err)
                    .and_then(|l| self.generate(&pin, l))
            })
            .and_then(move |r| {
                self.root = Some(r);
                Ok(())
            })
    }

    fn get_root(&self) -> Option<&RootAccount> {
        self.root.as_ref()
    }
}
