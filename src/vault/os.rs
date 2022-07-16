use core::{future::Ready, iter::repeat, ops::Deref};

use crate::{
    mnemonic::{Language, Mnemonic},
    RootAccount, Vault,
};

use arrayvec::ArrayString;
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
        use hmac::Hmac;
        use pbkdf2::pbkdf2;
        use sha2::Sha512;

        let phrase = self
            .get()?
            .parse::<Mnemonic>()
            .map_err(|_| Error::BadPhrase)?;

        // using same hashing strategy as Substrate to have some compatibility
        let mut salt = ArrayString::<{ 8 + PIN_LEN }>::new_const();
        let _len = pin.eq(&0).then_some(0).unwrap_or(PIN_LEN);
        salt.push_str("mnemonic");
        salt.push_str("");
        let mut seed = [0; 64];
        pbkdf2::<Hmac<Sha512>>(phrase.entropy(), salt.as_bytes(), 2048, &mut seed);

        println!("{:x?}", &seed);
        Ok(RootAccount::from_bytes(&seed))
    }

    // Create new random seed and save it in the OS keyring.
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
    type AuthDone = Ready<Result<(), Self::Error>>;
    type Error = Error;

    fn unlock(&mut self, pin: impl Into<Self::Credentials>) -> Self::AuthDone {
        let pin = pin.into();
        // TODO make truly async
        let res = self
            .get_key(&pin)
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

pub struct Pin(u16);

const PIN_LEN: usize = 4;

// Use 4 chars long hex string as pin. i.e. "ABCD", "1234"
impl<'a> From<&'a str> for Pin {
    fn from(s: &str) -> Self {
        let n = s.len().min(PIN_LEN);
        let chars = s.chars().take(n).chain(repeat('0').take(PIN_LEN - n));
        Pin(chars
            .map(|c| c.to_digit(16).unwrap_or(0))
            .enumerate()
            .fold(0, |pin, (i, d)| pin | ((d as u16) << i * PIN_LEN)))
    }
}

impl From<()> for Pin {
    fn from(_: ()) -> Self {
        Self(0)
    }
}

impl From<u16> for Pin {
    fn from(n: u16) -> Self {
        Self(n)
    }
}

impl Deref for Pin {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
