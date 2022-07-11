use core::future::Ready;

use crate::{
    mnemonic::{Language, Mnemonic},
    RootAccount, Vault,
};

use keyring;

const SERVICE: &str = "libwallet_account";

pub struct OSVault {
    entry: keyring::Entry,
    root: Option<RootAccount>,
    auto_generate: Option<Language>,
}

impl OSVault {
    // Make new OSVault from entry with name.
    // Doesn't save any password.
    // If password doesn't exist in the system, it will fail later.
    pub fn new(uname: &str, lang: impl Into<Option<Language>>) -> Self {
        OSVault {
            entry: keyring::Entry::new(SERVICE, &uname),
            root: None,
            auto_generate: lang.into(),
        }
    }

    // Create new password saved in OS with given name.
    // Save seed as password in the OS.
    pub fn update(&self, phrase: &str) -> Result<(), ()> {
        self.entry.set_password(phrase).map_err(|_| ())
    }

    pub fn get(&self) -> Result<String, Error> {
        self.entry
            .get_password()
            // .inspect_err(|e| {
            //     dbg!(e);
            // })
            .map_err(|_| Error::Keyring)
    }

    fn get_key_pair(&self) -> Result<RootAccount, Error> {
        let phrase = self
            .get()?
            .parse::<Mnemonic>()
            .map_err(|_| Error::NotFound)?;
        Ok(RootAccount::from_bytes(phrase.entropy()))
    }

    /// Create new random seed and save it in the OS keyring.
    fn generate(&self, lang: Language) -> Result<RootAccount, Error> {
        let (root, phrase) = RootAccount::generate_with_phrase(&mut rand_core::OsRng, lang);
        self.entry
            .set_password(phrase.phrase())
            // .inspect_err(|e| {
            //     dbg!(e);
            // })
            .map_err(|_| Error::Keyring)?;
        Ok(root)
    }
}

#[derive(Debug)]
pub enum Error {
    Keyring,
    NotFound,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::Keyring => write!(f, "OS Key storage error"),
            Error::NotFound => write!(f, "Key not found"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl Vault for OSVault {
    type Credentials = ();
    type AuthDone = Ready<Result<(), Self::Error>>;
    type Error = Error;

    fn unlock(&mut self, _c: ()) -> Self::AuthDone {
        // TODO make truly async
        let res = self
            .get_key_pair()
            .or_else(|err| self.auto_generate.ok_or(err).and_then(|l| self.generate(l)))
            .and_then(move |r| {
                self.root = Some(r);
                Ok(())
            });
        core::future::ready(res)
    }

    fn get_root(&self) -> Option<&RootAccount> {
        self.root.as_ref()
    }
}
