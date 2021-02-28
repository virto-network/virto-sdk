use async_trait::async_trait;
use frame_metadata::{RuntimeMetadata, StorageEntryType, StorageHasher};
use hasher::hash;
use meta_ext::MetaExt;
use std::{
    convert::{TryFrom, TryInto},
    fmt,
    marker::PhantomData,
    ops::Deref,
    str::FromStr,
};

#[cfg(feature = "http")]
pub mod http;

mod hasher;
mod meta_ext;

/// Submit extrinsics
pub struct Sube<T>(T);

impl<T: Backend> From<T> for Sube<T> {
    fn from(b: T) -> Self {
        Sube(b)
    }
}

impl<T> Deref for Sube<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Generic backend definition
#[async_trait]
pub trait Backend {
    /// Get storage items form the blockchain
    async fn query<K>(&self, key: K) -> Result<String>
    where
        K: TryInto<StorageKey<Self>> + Send;

    /// Send a signed extrinsic to the blockchain
    async fn submit(ext: &[u8]) -> Result<()>;

    fn get_metadata() -> &'static RuntimeMetadata {
        todo!()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub enum Error {
    BadKey,
    NotFound,
    ParseStorageItem,
}

#[derive(Clone, Debug)]
pub struct StorageKey<T: ?Sized>(Vec<u8>, PhantomData<T>);

impl<T: Backend> StorageKey<T> {
    fn from_parts(module: &str, item: &str, k1: Option<&str>, k2: Option<&str>) -> Result<Self> {
        let meta = T::get_metadata();
        let entry = meta.entry(module, item).ok_or(Error::NotFound)?;
        let mut key = hash(&StorageHasher::Twox128, &module);
        key.append(&mut hash(&StorageHasher::Twox128, &entry.name.to_string()));

        let key = match entry.ty {
            StorageEntryType::Plain(_) => key,
            StorageEntryType::Map { ref hasher, .. } => {
                if k1.is_none() || k1.as_ref().unwrap().is_empty() {
                    return Err(Error::NotFound);
                }
                key.append(&mut hash(hasher, &k1.unwrap()));
                key
            }
            StorageEntryType::DoubleMap {
                ref hasher,
                ref key2_hasher,
                ..
            } => {
                if (k1.is_none() || k1.as_ref().unwrap().is_empty())
                    || (k2.is_none() || k2.as_ref().unwrap().is_empty())
                {
                    return Err(Error::NotFound);
                }
                key.append(&mut hash(hasher, &k1.unwrap()));
                key.append(&mut hash(key2_hasher, &k2.unwrap()));
                key
            }
        };

        Ok(StorageKey(key, PhantomData))
    }
}

impl<T: Backend> FromStr for StorageKey<T> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        // asume it's a path like "module/item-name"
        let mut path = s.split('/');
        let module = path.next().map(to_camel).ok_or(Error::ParseStorageItem)?;
        let item = path.next().map(to_camel).ok_or(Error::ParseStorageItem)?;
        let k1 = path.next();
        let k2 = path.next();
        StorageKey::from_parts(&module, &item, k1, k2)
    }
}

impl<T: Backend> TryFrom<&str> for StorageKey<T> {
    type Error = Error;
    fn try_from(s: &str) -> Result<Self> {
        s.parse()
    }
}

impl<T: Backend, U: AsRef<str>> From<(U, U)> for StorageKey<T> {
    fn from((m, it): (U, U)) -> Self {
        StorageKey::from_parts(m.as_ref(), it.as_ref(), None, None)
            .expect("valid module and item names")
    }
}

impl<M> fmt::Display for StorageKey<M> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0))
    }
}

impl<T> AsRef<[u8]> for StorageKey<T> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

fn to_camel(term: &str) -> String {
    let underscore_count = term.chars().filter(|c| *c == '-').count();
    let mut result = String::with_capacity(term.len() - underscore_count);
    let mut at_new_word = true;

    for c in term.chars().skip_while(|&c| c == '-') {
        if c == '-' {
            at_new_word = true;
        } else if at_new_word {
            result.push(c.to_ascii_uppercase());
            at_new_word = false;
        } else {
            result.push(c);
        }
    }
    result
}
