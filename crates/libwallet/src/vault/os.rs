use core::marker::PhantomData;

use keyring;
use mnemonic::{Language, Mnemonic};

const SERVICE: &str = "libwallet_account";

/// A key store backed by the OS secure store (keychain/credential manager).
/// Wrap with `Substrate<OSKeyring<..>>` to produce substrate-compatible signers.
pub struct OSKeyring<S> {
    entry: keyring::Entry,
    entropy: Option<zeroize::Zeroizing<Vec<u8>>>,
    auto_generate: Option<Language>,
    _phantom: PhantomData<S>,
}

impl<S> OSKeyring<S> {
    pub fn new(uname: &str, lang: impl Into<Option<Language>>) -> Self {
        OSKeyring {
            entry: keyring::Entry::new(SERVICE, uname),
            entropy: None,
            auto_generate: lang.into(),
            _phantom: PhantomData,
        }
    }

    pub fn update(&self, phrase: &str) -> Result<(), Error> {
        self.entry.set_password(phrase).map_err(|_| Error::Keyring)
    }

    pub(crate) fn get(&self) -> Result<zeroize::Zeroizing<String>, Error> {
        self.entry
            .get_password()
            .map(zeroize::Zeroizing::new)
            .map_err(|error| match error {
                keyring::Error::NoEntry => Error::NotFound,
                _ => Error::Keyring,
            })
    }

    fn load_entropy(&mut self) -> Result<(), Error> {
        let stored = self
            .get()
            .or_else(|err| self.auto_generate.ok_or(err).and_then(|l| self.generate(l)))?;

        if let Some(raw) = crate::chain::decode_raw_secret(&stored) {
            self.entropy = Some(zeroize::Zeroizing::new(raw));
            return Ok(());
        }

        let mnemonic = stored.parse::<Mnemonic>().map_err(|_| Error::BadPhrase)?;

        self.entropy = Some(zeroize::Zeroizing::new(mnemonic.entropy().to_vec()));
        Ok(())
    }

    fn generate(&self, lang: Language) -> Result<zeroize::Zeroizing<String>, Error> {
        let phrase = crate::util::gen_phrase(&mut rand_core::OsRng, lang);
        self.entry
            .set_password(phrase.phrase())
            .map_err(|_| Error::Keyring)?;
        Ok(zeroize::Zeroizing::new(phrase.phrase().to_string()))
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

impl<S> crate::chain::KeyStore for OSKeyring<S> {
    type Error = Error;
    fn unlock(&mut self) -> Result<&[u8], Self::Error> {
        if self.entropy.is_none() {
            self.load_entropy()?;
        }
        self.entropy
            .as_deref()
            .map(Vec::as_slice)
            .ok_or(Error::NotFound)
    }
}

impl<S> crate::chain::MutableKeyStore for OSKeyring<S> {
    type Error = Error;

    fn upsert(&mut self, secret: &[u8]) -> Result<(), Self::Error> {
        let encoded = crate::chain::encode_raw_secret(secret);
        self.entry
            .set_password(&encoded)
            .map_err(|_| Error::Keyring)?;
        self.entropy = None;
        Ok(())
    }

    fn delete(&mut self) -> Result<(), Self::Error> {
        match self.entry.delete_password() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => return Err(Error::Keyring),
        }
        self.entropy = None;
        Ok(())
    }
}
