use crate::{async_trait, Box, Pair, Result, Vault, Error};

use keyring;


pub struct OSVault<P: Pair> {
    entry: keyring::Entry,
    seed: Option<P::Seed>,
}

impl<P: Pair> OSVault<P> {
    /// Create new entry with a random seed and save it in the OS.
    pub fn create(name: &str) -> Result<(Self, String)> {
        let entry = keyring::Entry::new("wallet", &name);
        let (_, phrase, seed) = P::generate_with_phrase(None);
        entry.set_password(&phrase).map_err(|_| Error::InvalidPhrase)?;
        Ok((OSVault { 
            entry, 
            seed: Some(seed) 
        }, phrase))
    }
    
    // Create new password saved in OS with given name.
    // Save seed as password in the OS.
    pub fn create_with_seed(name: &str, seed: &str) -> Result<Self> {
        let entry = keyring::Entry::new("wallet", &name);
        entry.set_password(seed).map_err(|_| Error::InvalidPhrase)?;
        Ok(OSVault {
            entry,
            seed: None,
        })
    }

    // Make new OSVault from entry with name. 
    // Doesn't save any password.
    // If password doesn't exist in the system, it will fail later.
    pub fn new(name: &str) -> Self {
        OSVault { 
            entry: keyring::Entry::new("wallet", &name), 
            seed: None 
        }
    }  
}

#[async_trait(?Send)]
impl<P: Pair> Vault for OSVault<P> {
    type Pair = P;

    async fn unlock(&mut self, _: ()) -> Result<P> {
        // get seed from entry
        match self.entry.get_password() {
            Ok(s) => match P::from_phrase(&s, None) {
                Ok((pair, seed)) => {
                    self.seed = Some(seed);
                    Ok(pair)
                },
                Err(_) => Err(Error::InvalidPhrase),
            },
            Err(_) => Err(Error::InvalidPhrase),
        }
    }
}

