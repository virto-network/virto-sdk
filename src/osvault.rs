use core::future::Ready;

use crate::{
    mnemonic::{Language, Mnemonic},
    Error, Pair, Result, Vault,
};

use keyring;

const SERVICE: &str = "libwallet_account";

pub struct OSVault<P: Pair> {
    entry: keyring::Entry,
    seed: Option<P::Seed>,
}

impl<P: Pair> OSVault<P> {
    // Make new OSVault from entry with name.
    // Doesn't save any password.
    // If password doesn't exist in the system, it will fail later.
    pub fn new(uname: &str) -> Self {
        OSVault {
            entry: keyring::Entry::new(SERVICE, &uname),
            seed: None,
        }
    }

    /// Create new random seed and save it in the OS keyring.
    pub fn generate(self) -> Result<(Self, Mnemonic)> {
        let (_, phrase) = P::generate_with_phrase(Language::default());
        self.entry
            .set_password(phrase.phrase())
            // .inspect_err(|e| {
            //     dbg!(e);
            // })
            .map_err(|_| Error::InvalidPhrase)?;
        Ok((self, phrase))
    }

    // pub async fn get_or_generate(&self) -> Result<Self> {}

    // Create new password saved in OS with given name.
    // Save seed as password in the OS.
    pub fn update(&self, phrase: &str) -> Result<()> {
        self.entry
            .set_password(phrase)
            .map_err(|_| Error::InvalidPhrase)
    }

    fn get_key_pair(&self) -> Option<(P, P::Seed)> {
        let phrase = self
            .entry
            .get_password()
            // .inspect_err(|e| {
            //     dbg!(e);
            // })
            .ok()?;
        let phrase = phrase.parse::<Mnemonic>().ok()?;
        Some(P::from_bytes(phrase.entropy()))
    }
}

impl<P: Pair> Vault for OSVault<P> {
    type Pair = P;
    type PairFut = Ready<Result<Self::Pair>>;

    fn unlock<C>(&mut self, _c: C) -> Self::PairFut {
        // TODO make truly async
        let res = self
            .get_key_pair()
            .and_then(|(p, s)| {
                self.seed = Some(s);
                Some(p)
            })
            .ok_or_else(|| Error::InvalidPhrase);
        core::future::ready(res)
    }
}
