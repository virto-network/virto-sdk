#![feature(prelude_import)]
#![feature(async_fn_in_trait, impl_trait_projections)]
//! `libwallet` is the one-stop tool to build easy, slightly opinionated crypto wallets
//! that run in all kinds of environments and plattforms including embedded hardware,
//! mobile apps or the Web.
//! It's easy to extend implementing different vault backends and it's designed to
//! be compatible with all kinds of key formats found in many different blockchains.
#[prelude_import]
use std::prelude::rust_2018::*;
#[macro_use]
extern crate std;
mod account {
    use crate::{
        any::{self, AnySignature},
        Derive, Network, Pair, Public, RootAccount,
    };
    use arrayvec::ArrayString;
    const MAX_PATH_LEN: usize = 16;
    /// Account is an abstration around public/private key pairs that are more convenient to use and
    /// can hold extra metadata. Accounts are constructed by the wallet and are used to sign messages.
    pub struct Account {
        pair: Option<any::Pair>,
        network: Network,
        path: ArrayString<MAX_PATH_LEN>,
        name: ArrayString<{ MAX_PATH_LEN - 2 }>,
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for Account {
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field4_finish(
                f,
                "Account",
                "pair",
                &self.pair,
                "network",
                &self.network,
                "path",
                &self.path,
                "name",
                &&self.name,
            )
        }
    }
    impl Account {
        pub(crate) fn new<'a>(name: impl Into<Option<&'a str>>) -> Self {
            let n = name.into().unwrap_or_else(|| "default");
            let mut path = ArrayString::from("//").unwrap();
            path.push_str(&n);
            Account {
                pair: None,
                network: Network::default(),
                name: ArrayString::from(&n).expect("short name"),
                path,
            }
        }
        pub fn switch_network(self, net: impl Into<Network>) -> Self {
            Account {
                network: net.into(),
                ..self
            }
        }
        pub fn name(&self) -> &str {
            &self.name
        }
        pub fn public(&self) -> impl Public {
            self.pair.as_ref().expect("account unlocked").public()
        }
        pub fn network(&self) -> &Network {
            &self.network
        }
        pub fn is_locked(&self) -> bool {
            self.pair.is_none()
        }
        pub(crate) fn unlock(&mut self, root: &RootAccount) -> &Self {
            if self.is_locked() {
                self.pair = Some(root.derive(&self.path));
            }
            self
        }
    }
    impl crate::Signer for Account {
        type Signature = AnySignature;
        fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
            self.pair.as_ref().expect("account unlocked").sign_msg(msg)
        }
        fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool {
            self.pair.as_ref().expect("account unlocked").verify(msg, sig)
        }
    }
    #[cfg(feature = "serde")]
    impl serde::Serialize for Account {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            use serde::ser::SerializeStruct;
            let mut state = serializer.serialize_struct("Account", 1)?;
            state.serialize_field("network", &self.network)?;
            state.serialize_field("path", self.path.as_str())?;
            state.serialize_field("name", self.name.as_str())?;
            state.end()
        }
    }
    impl core::fmt::Display for Account {
        fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            for byte in self.public().as_ref() {
                f.write_fmt(format_args!("{0:02x}", byte))?;
            }
            Ok(())
        }
    }
}
mod key_pair {
    use core::fmt::Debug;
    pub use derive::Derive;
    type Bytes<const N: usize> = [u8; N];
    /// A key pair with a public key
    pub trait Pair: Signer + Derive {
        type Public: Public;
        fn from_bytes(seed: &[u8]) -> Self
        where
            Self: Sized;
        fn public(&self) -> Self::Public;
    }
    pub trait Public: AsRef<[u8]> + Debug {}
    impl<const N: usize> Public for Bytes<N> {}
    pub trait Signature: AsRef<[u8]> + Debug + PartialEq {}
    impl<const N: usize> Signature for Bytes<N> {}
    /// Something that can sign messages
    pub trait Signer {
        type Signature: Signature;
        fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature;
        fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool;
    }
    /// Wrappers to represent any supported key pair.
    pub mod any {
        use super::{Public, Signature};
        use core::fmt;
        pub enum Pair {
            #[cfg(feature = "sr25519")]
            Sr25519(super::sr25519::Pair),
        }
        #[automatically_derived]
        impl ::core::fmt::Debug for Pair {
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                match self {
                    Pair::Sr25519(__self_0) => {
                        ::core::fmt::Formatter::debug_tuple_field1_finish(
                            f,
                            "Sr25519",
                            &__self_0,
                        )
                    }
                }
            }
        }
        impl super::Pair for Pair {
            type Public = AnyPublic;
            fn from_bytes(seed: &[u8]) -> Self
            where
                Self: Sized,
            {
                #[cfg(feature = "sr25519")]
                let p = Self::Sr25519(
                    <super::sr25519::Pair as super::Pair>::from_bytes(seed),
                );
                p
            }
            fn public(&self) -> Self::Public {
                match self {
                    Pair::Sr25519(p) => AnyPublic::Sr25519(p.public()),
                }
            }
        }
        impl super::Derive for Pair {
            type Pair = Pair;
            fn derive(&self, path: &str) -> Self::Pair
            where
                Self: Sized,
            {
                match self {
                    Pair::Sr25519(kp) => Pair::Sr25519(kp.derive(path)),
                }
            }
        }
        #[cfg(feature = "sr25519")]
        impl From<super::sr25519::Pair> for Pair {
            fn from(p: super::sr25519::Pair) -> Self {
                Self::Sr25519(p)
            }
        }
        impl super::Signer for Pair {
            type Signature = AnySignature;
            fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
                match self {
                    #[cfg(feature = "sr25519")]
                    Pair::Sr25519(p) => p.sign_msg(msg).into(),
                }
            }
            fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool {
                match self {
                    Pair::Sr25519(p) => super::Signer::verify(p, msg, sig),
                }
            }
        }
        pub enum AnyPublic {
            #[cfg(feature = "sr25519")]
            Sr25519(super::Bytes<{ super::sr25519::SEED_LEN }>),
        }
        #[automatically_derived]
        impl ::core::fmt::Debug for AnyPublic {
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                match self {
                    AnyPublic::Sr25519(__self_0) => {
                        ::core::fmt::Formatter::debug_tuple_field1_finish(
                            f,
                            "Sr25519",
                            &__self_0,
                        )
                    }
                }
            }
        }
        impl AsRef<[u8]> for AnyPublic {
            fn as_ref(&self) -> &[u8] {
                match self {
                    AnyPublic::Sr25519(p) => p.as_ref(),
                }
            }
        }
        impl fmt::Display for AnyPublic {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                for b in self.as_ref() {
                    f.write_fmt(format_args!("{0:02X}", b))?;
                }
                Ok(())
            }
        }
        impl Public for AnyPublic {}
        pub enum AnySignature {
            #[cfg(feature = "sr25519")]
            Sr25519(super::Bytes<{ super::sr25519::SIG_LEN }>),
        }
        #[automatically_derived]
        impl ::core::fmt::Debug for AnySignature {
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                match self {
                    AnySignature::Sr25519(__self_0) => {
                        ::core::fmt::Formatter::debug_tuple_field1_finish(
                            f,
                            "Sr25519",
                            &__self_0,
                        )
                    }
                }
            }
        }
        #[automatically_derived]
        impl ::core::marker::StructuralPartialEq for AnySignature {}
        #[automatically_derived]
        impl ::core::cmp::PartialEq for AnySignature {
            #[inline]
            fn eq(&self, other: &AnySignature) -> bool {
                match (self, other) {
                    (
                        AnySignature::Sr25519(__self_0),
                        AnySignature::Sr25519(__arg1_0),
                    ) => *__self_0 == *__arg1_0,
                }
            }
        }
        impl AsRef<[u8]> for AnySignature {
            fn as_ref(&self) -> &[u8] {
                match self {
                    #[cfg(feature = "sr25519")]
                    AnySignature::Sr25519(s) => s.as_ref(),
                }
            }
        }
        #[cfg(feature = "sr25519")]
        impl From<super::sr25519::Signature> for AnySignature {
            fn from(s: super::sr25519::Signature) -> Self {
                AnySignature::Sr25519(s)
            }
        }
        impl Signature for AnySignature {}
    }
    #[cfg(feature = "sr25519")]
    pub mod sr25519 {
        use super::{derive::Junction, Bytes, Derive, Signer};
        use schnorrkel::{
            derive::{ChainCode, Derivation},
            signing_context, ExpansionMode, MiniSecretKey, SecretKey,
            MINI_SECRET_KEY_LENGTH,
        };
        pub use schnorrkel::Keypair as Pair;
        pub(super) const SEED_LEN: usize = MINI_SECRET_KEY_LENGTH;
        pub(super) const SIG_LEN: usize = 64;
        pub type Seed = Bytes<SEED_LEN>;
        pub type Public = Bytes<32>;
        pub type Signature = Bytes<64>;
        const SIGNING_CTX: &[u8] = b"substrate";
        impl super::Pair for Pair {
            type Public = Public;
            fn from_bytes(bytes: &[u8]) -> Self {
                if !(bytes.len() >= SEED_LEN) {
                    ::core::panicking::panic("assertion failed: bytes.len() >= SEED_LEN")
                }
                let minikey = MiniSecretKey::from_bytes(&bytes[..SEED_LEN]).unwrap();
                minikey.expand_to_keypair(ExpansionMode::Ed25519)
            }
            fn public(&self) -> Self::Public {
                let mut key = [0u8; 32];
                key.copy_from_slice(self.public.as_ref());
                key
            }
        }
        impl Signer for Pair {
            type Signature = Signature;
            fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Self::Signature {
                let context = signing_context(SIGNING_CTX);
                self.sign(context.bytes(msg.as_ref())).to_bytes()
            }
            fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool {
                let sig = match schnorrkel::Signature::from_bytes(sig) {
                    Ok(s) => s,
                    Err(_) => return false,
                };
                self.public.verify_simple(SIGNING_CTX, msg.as_ref(), &sig).is_ok()
            }
        }
        impl Derive for Pair {
            type Pair = Self;
            fn derive(&self, path: &str) -> Self
            where
                Self: Sized,
            {
                super::derive::parse_substrate_junctions(path)
                    .fold(
                        self.secret.clone(),
                        |key, (part, hard)| {
                            if hard {
                                key.hard_derive_mini_secret_key(Some(ChainCode(part)), &[])
                                    .0
                                    .expand(ExpansionMode::Ed25519)
                            } else {
                                derive_simple(key, part)
                            }
                        },
                    )
                    .into()
            }
        }
        #[cfg(feature = "rand_chacha")]
        fn derive_simple(key: SecretKey, j: Junction) -> SecretKey {
            use rand_core::SeedableRng;
            let rng = rand_chacha::ChaChaRng::from_seed([0; 32]);
            key.derived_key_simple_rng(ChainCode(j), &[], rng).0
        }
    }
    mod derive {
        use core::convert::identity;
        use super::Bytes;
        /// Something to derive key pairs form
        pub trait Derive {
            type Pair: super::Signer;
            fn derive(&self, path: &str) -> Self::Pair
            where
                Self: Sized;
        }
        const JUNCTION_LEN: usize = 32;
        pub(super) type Junction = Bytes<JUNCTION_LEN>;
        pub(super) fn parse_substrate_junctions(
            path: &str,
        ) -> impl Iterator<Item = (Junction, bool)> + '_ {
            path.split_inclusive("/")
                .flat_map(|s| if s == "/" { "" } else { s }.split("/"))
                .scan(
                    0u8,
                    |j, part| {
                        Some(
                            if part.is_empty() {
                                *j += 1;
                                None
                            } else {
                                let hard = *j > 1;
                                *j = 0;
                                Some((encoded_junction(part), hard))
                            },
                        )
                    },
                )
                .filter_map(identity)
        }
        fn encoded_junction(part: &str) -> Junction {
            let mut code = [0; JUNCTION_LEN];
            if let Ok(n) = part.parse::<u64>() {
                code[..8].copy_from_slice(&n.to_le_bytes());
            } else {
                let len = part.len().min(JUNCTION_LEN - 1);
                code[0] = (len as u8) << 2;
                code[1..len + 1].copy_from_slice(&part[..len].as_bytes());
            }
            code
        }
    }
}
#[cfg(feature = "substrate")]
mod substrate_ext {
    use crate::Network;
    trait SubstrateExt {}
    impl From<&str> for Network {
        fn from(s: &str) -> Self {
            match s {
                "polkadot" => Network::Substrate(0),
                "kusama" => Network::Substrate(2),
                "karura" => Network::Substrate(8),
                "substrate" => Network::Substrate(42),
                _ => Network::default(),
            }
        }
    }
}
use arrayvec::ArrayVec;
use core::{convert::TryInto, fmt};
use key_pair::any::AnySignature;
#[cfg(feature = "mnemonic")]
use mnemonic;
pub use account::Account;
pub use key_pair::*;
#[cfg(feature = "mnemonic")]
pub use mnemonic::{Language, Mnemonic};
pub use vault::{RootAccount, Vault};
pub mod vault {
    //! Collection of supported Vault backends
    #[cfg(feature = "vault_os")]
    mod os {
        use crate::{
            mnemonic::{Language, Mnemonic},
            util::{Pin, seed_from_entropy},
            RootAccount, Vault,
        };
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
                self.entry.get_password().map_err(|_| Error::Keyring)
            }
            fn get_key(&self, pin: Pin) -> Result<RootAccount, Error> {
                let phrase = self
                    .get()?
                    .parse::<Mnemonic>()
                    .map_err(|_| Error::BadPhrase)?;
                let seed = {
                    let seed = &phrase.entropy();
                    #[cfg(feature = "util_pin")]
                    let protected_seed = { pin.protect::<64>(seed) };
                    #[cfg(feature = "util_pin")]
                    let seed = &protected_seed;
                    seed
                };
                Ok(RootAccount::from_bytes(&seed))
            }
            fn generate(&self, pin: Pin, lang: Language) -> Result<RootAccount, Error> {
                let phrase = crate::util::gen_phrase(&mut rand_core::OsRng, lang);
                let seed = {
                    let seed = &phrase.entropy();
                    #[cfg(feature = "util_pin")]
                    let protected_seed = { pin.protect::<64>(seed) };
                    #[cfg(feature = "util_pin")]
                    let seed = &protected_seed;
                    seed
                };
                let root = RootAccount::from_bytes(&seed);
                self.entry
                    .set_password(phrase.phrase())
                    .map_err(|e| {
                        match e {
                            tmp => {
                                {
                                    ::std::io::_eprint(
                                        format_args!(
                                            "[{0}:{1}] {2} = {3:#?}\n", "src/vault/os.rs", 67u32, "e", &
                                            tmp
                                        ),
                                    );
                                };
                                tmp
                            }
                        };
                        Error::Keyring
                    })?;
                Ok(root)
            }
        }
        pub enum Error {
            Keyring,
            NotFound,
            BadPhrase,
        }
        #[automatically_derived]
        impl ::core::fmt::Debug for Error {
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                ::core::fmt::Formatter::write_str(
                    f,
                    match self {
                        Error::Keyring => "Keyring",
                        Error::NotFound => "NotFound",
                        Error::BadPhrase => "BadPhrase",
                    },
                )
            }
        }
        impl core::fmt::Display for Error {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                match self {
                    Error::Keyring => f.write_fmt(format_args!("OS Key storage error")),
                    Error::NotFound => f.write_fmt(format_args!("Key not found")),
                    Error::BadPhrase => f.write_fmt(format_args!("Mnemonic is invalid")),
                }
            }
        }
        #[cfg(feature = "std")]
        impl std::error::Error for Error {}
        impl Vault for OSKeyring {
            type Credentials = Pin;
            type Error = Error;
            async fn unlock<T>(
                &mut self,
                pin: &Self::Credentials,
                mut cb: impl FnMut(&RootAccount) -> T,
            ) -> Result<T, Self::Error> {
                self.get_key(*pin)
                    .or_else(|err| {
                        self.auto_generate
                            .ok_or(err)
                            .and_then(|l| self.generate(*pin, l))
                    })
                    .and_then(|r| {
                        self.root = Some(r);
                        Ok(cb(self.root.as_ref().unwrap()))
                    })
            }
        }
    }
    #[cfg(feature = "vault_pass")]
    mod pass {
        use mnemonic::Language;
        use prs_lib::{
            crypto::{self, IsContext, Proto},
            store::{FindSecret, Store},
            Plaintext,
        };
        use crate::{
            util::{seed_from_entropy, Pin},
            RootAccount, Vault,
        };
        /// A vault that stores secrets in a `pass` compatible repository
        pub struct Pass {
            store: Store,
            root: Option<RootAccount>,
            auto_generate: Option<Language>,
        }
        const DEFAULT_DIR: &str = "libwallet_accounts/";
        impl Pass {
            /// Create a new `Pass` vault in the given location.
            /// The optional `lang` instructs the vault to generarte a backup phrase
            /// in the given language in case one does not exist.
            pub fn new<P: AsRef<str>>(
                store_path: P,
                lang: impl Into<Option<Language>>,
            ) -> Self {
                let store = Store::open(store_path).unwrap();
                Pass {
                    store,
                    root: None,
                    auto_generate: lang.into(),
                }
            }
            fn get_key(&self, credentials: &PassCreds) -> Result<RootAccount, Error> {
                let mut secret_path = String::from(DEFAULT_DIR);
                secret_path.push_str(&credentials.account);
                let secret = match self.store.find(Some(secret_path)) {
                    FindSecret::Exact(secret) => Some(secret),
                    FindSecret::Many(secrets) => secrets.first().cloned(),
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
                let seed = {
                    let seed = phrase;
                    #[cfg(feature = "util_pin")]
                    let protected_seed = {
                        credentials.pin.unwrap_or_default().protect::<64>(seed)
                    };
                    #[cfg(feature = "util_pin")]
                    let seed = &protected_seed;
                    seed
                };
                Ok(RootAccount::from_bytes(&seed))
            }
            #[cfg(all(feature = "rand", feature = "mnemonic"))]
            fn generate(
                &self,
                credentials: &PassCreds,
                lang: Language,
            ) -> Result<RootAccount, Error> {
                let map_encrypt_error = |e| {
                    match e {
                        tmp => {
                            {
                                ::std::io::_eprint(
                                    format_args!(
                                        "[{0}:{1}] {2} = {3:#?}\n", "src/vault/pass.rs", 63u32, "e",
                                        & tmp
                                    ),
                                );
                            };
                            tmp
                        }
                    };
                    Error::Encrypt
                };
                let phrase = crate::util::gen_phrase(&mut rand_core::OsRng, lang);
                let mut secret_path = String::from(DEFAULT_DIR);
                secret_path.push_str(&credentials.account);
                let secret_path = self
                    .store
                    .normalize_secret_path(secret_path, None, true)
                    .map_err(map_encrypt_error)?;
                let plaintext = Plaintext::from(phrase.to_string());
                crypto::context(Proto::Gpg)
                    .map_err(|_e| Error::Encrypt)?
                    .encrypt_file(
                        &self.store.recipients().map_err(map_encrypt_error)?,
                        plaintext,
                        &secret_path,
                    )
                    .map_err(map_encrypt_error)?;
                let seed = {
                    let seed = phrase;
                    #[cfg(feature = "util_pin")]
                    let protected_seed = {
                        credentials.pin.unwrap_or_default().protect::<64>(seed)
                    };
                    #[cfg(feature = "util_pin")]
                    let seed = &protected_seed;
                    seed
                };
                Ok(RootAccount::from_bytes(seed))
            }
        }
        pub enum Error {
            Store,
            NotFound,
            SecretPath,
            Encrypt,
            Decrypt,
            Plaintext,
        }
        #[automatically_derived]
        impl ::core::fmt::Debug for Error {
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                ::core::fmt::Formatter::write_str(
                    f,
                    match self {
                        Error::Store => "Store",
                        Error::NotFound => "NotFound",
                        Error::SecretPath => "SecretPath",
                        Error::Encrypt => "Encrypt",
                        Error::Decrypt => "Decrypt",
                        Error::Plaintext => "Plaintext",
                    },
                )
            }
        }
        impl core::fmt::Display for Error {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                match self {
                    Error::Store => f.write_fmt(format_args!("Store load error")),
                    Error::NotFound => f.write_fmt(format_args!("Secret not found")),
                    Error::SecretPath => {
                        f.write_fmt(format_args!("Could not unwrap the secret path"))
                    }
                    Error::Encrypt => {
                        f.write_fmt(format_args!("Could not encrypt the secret"))
                    }
                    Error::Decrypt => {
                        f.write_fmt(format_args!("Could not decrypt the secret"))
                    }
                    Error::Plaintext => {
                        f
                            .write_fmt(
                                format_args!("Could not generate or unwrap the plaintext"),
                            )
                    }
                }
            }
        }
        #[cfg(feature = "std")]
        impl std::error::Error for Error {}
        pub struct PassCreds {
            account: String,
            pin: Option<Pin>,
        }
        impl From<String> for PassCreds {
            fn from(account: String) -> Self {
                PassCreds {
                    account,
                    pin: Some(Pin::from("")),
                }
            }
        }
        impl Vault for Pass {
            type Credentials = PassCreds;
            type Error = Error;
            async fn unlock<T>(
                &mut self,
                creds: &Self::Credentials,
                mut cb: impl FnMut(&RootAccount) -> T,
            ) -> Result<T, Self::Error> {
                self.get_key(creds)
                    .or_else(|err| {
                        self.auto_generate
                            .ok_or(err)
                            .and_then(|l| self.generate(creds, l))
                    })
                    .and_then(|r| {
                        self.root = Some(r);
                        Ok(cb(self.root.as_ref().unwrap()))
                    })
            }
        }
    }
    #[cfg(feature = "vault_simple")]
    mod simple {
        use core::slice::SlicePattern;
        use crate::util::{seed_from_entropy, Pin};
        use crate::{RootAccount, Vault};
        /// A vault that holds secrets in memory
        pub struct Simple {
            locked: Option<Vec<u8>>,
            unlocked: Option<Vec<u8>>,
        }
        impl Simple {
            /// A vault with a random seed, once dropped the the vault can't be restored
            ///
            /// ```
            /// # use libwallet::{vault, Error, Derive, Pair, Vault};
            /// # type Result = std::result::Result<(), <vault::Simple as Vault>::Error>;
            /// # #[async_std::main] async fn main() -> Result {
            /// let mut vault = vault::Simple::generate(&mut rand_core::OsRng);
            /// let root = vault.unlock(None, |root| {
            ///     println!("{}", root.derive("//default").public());
            /// }).await?;
            /// # Ok(()) }
            /// ```
            #[cfg(feature = "rand")]
            pub fn generate<R>(rng: &mut R) -> Self
            where
                R: rand_core::CryptoRng + rand_core::RngCore,
            {
                let seed = &crate::util::random_bytes::<_, 32>(rng);
                Simple {
                    locked: Some(seed.to_vec()),
                    unlocked: None,
                }
            }
            #[cfg(all(feature = "rand", feature = "mnemonic"))]
            pub fn generate_with_phrase<R>(rng: &mut R) -> (Self, mnemonic::Mnemonic)
            where
                R: rand_core::CryptoRng + rand_core::RngCore,
            {
                let phrase = crate::util::gen_phrase(rng, Default::default());
                (
                    Simple {
                        locked: Some(phrase.entropy().to_vec()),
                        unlocked: None,
                    },
                    phrase,
                )
            }
            #[cfg(feature = "mnemonic")]
            pub fn from_phrase(phrase: impl AsRef<str>) -> Self {
                let phrase = phrase
                    .as_ref()
                    .parse::<mnemonic::Mnemonic>()
                    .expect("mnemonic");
                Simple {
                    locked: Some(phrase.entropy().to_vec()),
                    unlocked: None,
                }
            }
            fn get_key(&self, pin: Pin) -> Result<RootAccount, Error> {
                if let Some(entropy) = &self.unlocked {
                    let seed = {
                        let seed = entropy.as_slice();
                        #[cfg(feature = "util_pin")]
                        let protected_seed = { pin.protect::<64>(seed) };
                        #[cfg(feature = "util_pin")]
                        let seed = &protected_seed;
                        seed
                    };
                    Ok(RootAccount::from_bytes(seed))
                } else {
                    Err(Error)
                }
            }
        }
        pub struct Error;
        #[automatically_derived]
        impl ::core::fmt::Debug for Error {
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                ::core::fmt::Formatter::write_str(f, "Error")
            }
        }
        impl core::fmt::Display for Error {
            fn fmt(&self, _f: &mut core::fmt::Formatter) -> core::fmt::Result {
                Ok(())
            }
        }
        #[cfg(feature = "std")]
        impl std::error::Error for Error {}
        impl Vault for Simple {
            type Credentials = Option<Pin>;
            type Error = Error;
            async fn unlock<T>(
                &mut self,
                credentials: &Self::Credentials,
                mut cb: impl FnMut(&RootAccount) -> T,
            ) -> Result<T, Self::Error> {
                self.unlocked = self.locked.take();
                let pin = credentials.unwrap_or_default();
                let root_account = &self.get_key(pin)?;
                Ok(cb(root_account))
            }
        }
    }
    #[cfg(feature = "vault_os")]
    pub use os::*;
    #[cfg(feature = "vault_pass")]
    pub use pass::*;
    #[cfg(feature = "vault_simple")]
    pub use simple::*;
    use crate::{any, key_pair, Derive};
    /// Abstration for storage of private keys that are protected by some credentials.
    pub trait Vault {
        type Credentials;
        type Error;
        /// Use a set of credentials to make the guarded keys available to the user.
        /// It returns a `Future` to allow for vaults that might take an arbitrary amount
        /// of time getting the secret ready like waiting for some user physical interaction.
        async fn unlock<T>(
            &mut self,
            cred: &Self::Credentials,
            cb: impl FnMut(&RootAccount) -> T,
        ) -> Result<T, Self::Error>;
    }
    /// The root account is a container of the key pairs stored in the vault and cannot be
    /// used to sign messages directly, we always derive new key pairs from it to create
    /// and use accounts with the wallet.
    pub struct RootAccount {
        #[cfg(feature = "substrate")]
        sub: key_pair::sr25519::Pair,
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for RootAccount {
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field1_finish(
                f,
                "RootAccount",
                "sub",
                &&self.sub,
            )
        }
    }
    impl RootAccount {
        fn from_bytes(seed: &[u8]) -> Self {
            RootAccount {
                #[cfg(feature = "substrate")]
                sub: <key_pair::sr25519::Pair as crate::Pair>::from_bytes(seed),
            }
        }
    }
    impl<'a> Derive for &'a RootAccount {
        type Pair = any::Pair;
        fn derive(&self, path: &str) -> Self::Pair
        where
            Self: Sized,
        {
            match &path[..2] {
                #[cfg(feature = "substrate")]
                "//" => self.sub.derive(path).into(),
                "m/" => ::core::panicking::panic("not implemented"),
                #[cfg(feature = "substrate")]
                _ => self.sub.derive("//default").into(),
            }
        }
    }
}
const MSG_MAX_SIZE: usize = u8::MAX as usize;
type Message = ArrayVec<u8, { MSG_MAX_SIZE }>;
/// Wallet is the main interface to interact with the accounts of a user.
///
/// Before being able to sign messages a wallet must be unlocked using valid credentials
/// supported by the underlying vault.
///
/// Wallets can hold many user defined accounts and always have one account set as "default",
/// if no account is set as default one is generated and will be used to sign messages when no account is specified.
///
/// Wallets also support queuing and bulk signing of messages in case transactions need to be reviewed before signing.
pub struct Wallet<V: Vault, const A: usize = 5, const M: usize = A> {
    vault: V,
    cached_creds: Option<V::Credentials>,
    default_account: Account,
    accounts: ArrayVec<Account, A>,
    pending_sign: ArrayVec<(Message, Option<u8>), M>,
}
#[automatically_derived]
impl<V: ::core::fmt::Debug + Vault, const A: usize, const M: usize> ::core::fmt::Debug
for Wallet<V, A, M>
where
    V::Credentials: ::core::fmt::Debug,
{
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field5_finish(
            f,
            "Wallet",
            "vault",
            &self.vault,
            "cached_creds",
            &self.cached_creds,
            "default_account",
            &self.default_account,
            "accounts",
            &self.accounts,
            "pending_sign",
            &&self.pending_sign,
        )
    }
}
impl<V, const A: usize, const M: usize> Wallet<V, A, M>
where
    V: Vault,
{
    /// Create a new Wallet with a default account
    pub fn new(vault: V) -> Self {
        Wallet {
            vault,
            default_account: Account::new(None),
            accounts: ArrayVec::new_const(),
            pending_sign: ArrayVec::new(),
            cached_creds: None,
        }
    }
    /// Get the account currently set as default
    pub fn default_account(&self) -> &Account {
        &self.default_account
    }
    /// Use credentials to unlock the vault.
    ///
    /// ```
    /// # use libwallet::{Wallet, Error, vault, Vault};
    /// # use std::convert::TryInto;
    /// # type Result = std::result::Result<(), Error<<vault::Simple as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = vault::Simple::generate(&mut rand_core::OsRng);
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// if wallet.is_locked() {
    ///     wallet.unlock(()).await?;
    /// }
    ///
    /// assert!(!wallet.is_locked());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unlock(
        &mut self,
        credentials: impl Into<V::Credentials>,
    ) -> Result<(), Error<V::Error>> {
        if self.is_locked() {
            let vault = &mut self.vault;
            let def = &mut self.default_account;
            let creds = credentials.into();
            vault
                .unlock(
                    &creds,
                    |root| {
                        def.unlock(root);
                    },
                )
                .await
                .map_err(|e| Error::Vault(e))?;
            self.cached_creds = Some(creds);
        }
        Ok(())
    }
    /// Check if the vault has been unlocked.
    pub fn is_locked(&self) -> bool {
        self.cached_creds.is_none()
    }
    /// Sign a message with the default account and return the signature.
    /// The wallet needs to be unlocked.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error, Signer, Vault};
    /// # type Result = std::result::Result<(), Error<<vault::Simple as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = vault::Simple::generate(&mut rand_core::OsRng);
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.unlock(()).await?;
    ///
    /// let msg = &[0x12, 0x34, 0x56];
    /// let signature = wallet.sign(msg);
    ///
    /// assert!(wallet.default_account().verify(msg, signature.as_ref()));
    /// # Ok(()) }
    /// ```
    pub fn sign(&self, message: &[u8]) -> impl Signature {
        if !!self.is_locked() {
            ::core::panicking::panic("assertion failed: !self.is_locked()")
        }
        self.default_account().sign_msg(message)
    }
    /// Save data to be signed some time later.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error, Vault};
    /// # type Result = std::result::Result<(), Error<<vault::Simple as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = vault::Simple::generate(&mut rand_core::OsRng);
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.sign_later(&[0x01, 0x02, 0x03]);
    ///
    /// assert_eq!(wallet.pending().count(), 1);
    /// # Ok(()) }
    /// ```
    pub fn sign_later<T>(&mut self, message: T)
    where
        T: AsRef<[u8]>,
    {
        let msg = message.as_ref();
        let msg = msg
            .try_into()
            .unwrap_or_else(|_| msg[..MSG_MAX_SIZE].try_into().unwrap());
        self.pending_sign.push((msg, None));
    }
    /// Try to sign all messages in the queue returning the list of signatures
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error, Vault};
    /// # type Result = std::result::Result<(), Error<<vault::Simple as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = vault::Simple::generate(&mut rand_core::OsRng);
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.unlock(()).await?;
    ///
    /// wallet.sign_later(&[0x01, 0x02, 0x03]);
    /// wallet.sign_later(&[0x04, 0x05, 0x06]);
    /// let signatures = wallet.sign_pending();
    ///
    /// assert_eq!(signatures.len(), 2);
    /// assert_eq!(wallet.pending().count(), 0);
    /// # Ok(()) }
    /// ```
    pub fn sign_pending(&mut self) -> ArrayVec<AnySignature, M> {
        let mut signatures = ArrayVec::new();
        for (msg, a) in self.pending_sign.take() {
            let account = a
                .map(|idx| self.account(idx))
                .unwrap_or_else(|| self.default_account());
            signatures.push(account.sign_msg(&msg));
        }
        signatures
    }
    /// Iteratate over the messages pending for signature for all the accounts.
    ///
    /// ```
    /// # use libwallet::{Wallet, vault, Error, Vault};
    /// # type Result = std::result::Result<(), Error<<vault::Simple as Vault>::Error>>;
    /// # #[async_std::main] async fn main() -> Result {
    /// # let vault = vault::Simple::generate(&mut rand_core::OsRng);
    /// let mut wallet: Wallet<_> = Wallet::new(vault);
    /// wallet.sign_later(&[0x01]);
    /// wallet.sign_later(&[0x02]);
    /// wallet.sign_later(&[0x03]);
    ///
    /// assert_eq!(wallet.pending().count(), 3);
    /// # Ok(()) }
    /// ```
    pub fn pending(&self) -> impl Iterator<Item = &[u8]> {
        self.pending_sign.iter().map(|(msg, _)| msg.as_ref())
    }
    fn account(&self, idx: u8) -> &Account {
        &self.accounts[idx as usize]
    }
}
/// Represents the blockchain network in use by an account
pub enum Network {
    #[cfg(feature = "substrate")]
    Substrate(u16),
    _Missing,
}
#[doc(hidden)]
#[allow(non_upper_case_globals, unused_attributes, unused_qualifications)]
const _: () = {
    #[allow(unused_extern_crates, clippy::useless_attribute)]
    extern crate serde as _serde;
    #[automatically_derived]
    impl _serde::Serialize for Network {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            match *self {
                Network::Substrate(ref __field0) => {
                    _serde::Serializer::serialize_newtype_variant(
                        __serializer,
                        "Network",
                        0u32,
                        "Substrate",
                        __field0,
                    )
                }
                Network::_Missing => {
                    _serde::Serializer::serialize_unit_variant(
                        __serializer,
                        "Network",
                        1u32,
                        "_Missing",
                    )
                }
            }
        }
    }
};
#[doc(hidden)]
#[allow(non_upper_case_globals, unused_attributes, unused_qualifications)]
const _: () = {
    #[allow(unused_extern_crates, clippy::useless_attribute)]
    extern crate serde as _serde;
    #[automatically_derived]
    impl<'de> _serde::Deserialize<'de> for Network {
        fn deserialize<__D>(
            __deserializer: __D,
        ) -> _serde::__private::Result<Self, __D::Error>
        where
            __D: _serde::Deserializer<'de>,
        {
            #[allow(non_camel_case_types)]
            enum __Field {
                __field0,
                __field1,
            }
            struct __FieldVisitor;
            impl<'de> _serde::de::Visitor<'de> for __FieldVisitor {
                type Value = __Field;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private::Formatter,
                ) -> _serde::__private::fmt::Result {
                    _serde::__private::Formatter::write_str(
                        __formatter,
                        "variant identifier",
                    )
                }
                fn visit_u64<__E>(
                    self,
                    __value: u64,
                ) -> _serde::__private::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        0u64 => _serde::__private::Ok(__Field::__field0),
                        1u64 => _serde::__private::Ok(__Field::__field1),
                        _ => {
                            _serde::__private::Err(
                                _serde::de::Error::invalid_value(
                                    _serde::de::Unexpected::Unsigned(__value),
                                    &"variant index 0 <= i < 2",
                                ),
                            )
                        }
                    }
                }
                fn visit_str<__E>(
                    self,
                    __value: &str,
                ) -> _serde::__private::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        "Substrate" => _serde::__private::Ok(__Field::__field0),
                        "_Missing" => _serde::__private::Ok(__Field::__field1),
                        _ => {
                            _serde::__private::Err(
                                _serde::de::Error::unknown_variant(__value, VARIANTS),
                            )
                        }
                    }
                }
                fn visit_bytes<__E>(
                    self,
                    __value: &[u8],
                ) -> _serde::__private::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        b"Substrate" => _serde::__private::Ok(__Field::__field0),
                        b"_Missing" => _serde::__private::Ok(__Field::__field1),
                        _ => {
                            let __value = &_serde::__private::from_utf8_lossy(__value);
                            _serde::__private::Err(
                                _serde::de::Error::unknown_variant(__value, VARIANTS),
                            )
                        }
                    }
                }
            }
            impl<'de> _serde::Deserialize<'de> for __Field {
                #[inline]
                fn deserialize<__D>(
                    __deserializer: __D,
                ) -> _serde::__private::Result<Self, __D::Error>
                where
                    __D: _serde::Deserializer<'de>,
                {
                    _serde::Deserializer::deserialize_identifier(
                        __deserializer,
                        __FieldVisitor,
                    )
                }
            }
            struct __Visitor<'de> {
                marker: _serde::__private::PhantomData<Network>,
                lifetime: _serde::__private::PhantomData<&'de ()>,
            }
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = Network;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private::Formatter,
                ) -> _serde::__private::fmt::Result {
                    _serde::__private::Formatter::write_str(__formatter, "enum Network")
                }
                fn visit_enum<__A>(
                    self,
                    __data: __A,
                ) -> _serde::__private::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::EnumAccess<'de>,
                {
                    match match _serde::de::EnumAccess::variant(__data) {
                        _serde::__private::Ok(__val) => __val,
                        _serde::__private::Err(__err) => {
                            return _serde::__private::Err(__err);
                        }
                    } {
                        (__Field::__field0, __variant) => {
                            _serde::__private::Result::map(
                                _serde::de::VariantAccess::newtype_variant::<
                                    u16,
                                >(__variant),
                                Network::Substrate,
                            )
                        }
                        (__Field::__field1, __variant) => {
                            match _serde::de::VariantAccess::unit_variant(__variant) {
                                _serde::__private::Ok(__val) => __val,
                                _serde::__private::Err(__err) => {
                                    return _serde::__private::Err(__err);
                                }
                            };
                            _serde::__private::Ok(Network::_Missing)
                        }
                    }
                }
            }
            const VARIANTS: &'static [&'static str] = &["Substrate", "_Missing"];
            _serde::Deserializer::deserialize_enum(
                __deserializer,
                "Network",
                VARIANTS,
                __Visitor {
                    marker: _serde::__private::PhantomData::<Network>,
                    lifetime: _serde::__private::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
impl ::core::fmt::Debug for Network {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        match self {
            Network::Substrate(__self_0) => {
                ::core::fmt::Formatter::debug_tuple_field1_finish(
                    f,
                    "Substrate",
                    &__self_0,
                )
            }
            Network::_Missing => ::core::fmt::Formatter::write_str(f, "_Missing"),
        }
    }
}
#[automatically_derived]
impl ::core::clone::Clone for Network {
    #[inline]
    fn clone(&self) -> Network {
        match self {
            Network::Substrate(__self_0) => {
                Network::Substrate(::core::clone::Clone::clone(__self_0))
            }
            Network::_Missing => Network::_Missing,
        }
    }
}
impl Default for Network {
    fn default() -> Self {
        #[cfg(feature = "substrate")]
        let net = Network::Substrate(42);
        net
    }
}
impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            #[cfg(feature = "substrate")]
            Self::Substrate(_) => f.write_fmt(format_args!("substrate")),
            _ => f.write_fmt(format_args!("")),
        }
    }
}
pub enum Error<V> {
    Vault(V),
    Locked,
    DeriveError,
    #[cfg(feature = "mnemonic")]
    InvalidPhrase,
}
#[automatically_derived]
impl<V: ::core::fmt::Debug> ::core::fmt::Debug for Error<V> {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        match self {
            Error::Vault(__self_0) => {
                ::core::fmt::Formatter::debug_tuple_field1_finish(f, "Vault", &__self_0)
            }
            Error::Locked => ::core::fmt::Formatter::write_str(f, "Locked"),
            Error::DeriveError => ::core::fmt::Formatter::write_str(f, "DeriveError"),
            Error::InvalidPhrase => ::core::fmt::Formatter::write_str(f, "InvalidPhrase"),
        }
    }
}
impl<V> fmt::Display for Error<V>
where
    V: fmt::Debug + fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Vault(e) => f.write_fmt(format_args!("Vault error: {0}", e)),
            Error::Locked => f.write_fmt(format_args!("Locked")),
            Error::DeriveError => f.write_fmt(format_args!("Cannot derive")),
            #[cfg(feature = "mnemonic")]
            Error::InvalidPhrase => f.write_fmt(format_args!("Invalid phrase")),
        }
    }
}
#[cfg(feature = "std")]
impl<V> std::error::Error for Error<V>
where
    V: fmt::Debug + fmt::Display,
{}
#[cfg(feature = "mnemonic")]
impl<V> From<mnemonic::Error> for Error<V> {
    fn from(_: mnemonic::Error) -> Self {
        Error::InvalidPhrase
    }
}
mod util {
    use core::{iter, ops};
    #[cfg(feature = "rand")]
    pub fn random_bytes<R, const S: usize>(rng: &mut R) -> [u8; S]
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let mut bytes = [0u8; S];
        rng.fill_bytes(&mut bytes);
        bytes
    }
    #[cfg(feature = "rand")]
    pub fn gen_phrase<R>(rng: &mut R, lang: mnemonic::Language) -> mnemonic::Mnemonic
    where
        R: rand_core::CryptoRng + rand_core::RngCore,
    {
        let seed = random_bytes::<_, 32>(rng);
        let phrase = mnemonic::Mnemonic::from_entropy_in(lang, seed.as_ref())
            .expect("seed valid");
        phrase
    }
    pub(crate) use seed_from_entropy;
    /// A simple pin credential that can be used to add some
    /// extra level of protection to seeds stored in vaults
    pub struct Pin(u16);
    #[automatically_derived]
    impl ::core::default::Default for Pin {
        #[inline]
        fn default() -> Pin {
            Pin(::core::default::Default::default())
        }
    }
    #[automatically_derived]
    impl ::core::marker::Copy for Pin {}
    #[automatically_derived]
    impl ::core::clone::Clone for Pin {
        #[inline]
        fn clone(&self) -> Pin {
            let _: ::core::clone::AssertParamIsClone<u16>;
            *self
        }
    }
    impl Pin {
        const LEN: usize = 4;
        #[cfg(feature = "util_pin")]
        pub fn protect<const S: usize>(&self, data: &[u8]) -> [u8; S] {
            use hmac::Hmac;
            use pbkdf2::pbkdf2;
            use sha2::Sha512;
            let salt = {
                let mut s = [0; 10];
                s.copy_from_slice(b"mnemonic\0\0");
                let [b1, b2] = self.to_le_bytes();
                s[8] = b1;
                s[9] = b2;
                s
            };
            let mut seed = [0; S];
            let len = self.eq(&0).then_some(salt.len() - 2).unwrap_or(salt.len());
            pbkdf2::<Hmac<Sha512>>(data, &salt[..len], 2048, &mut seed);
            seed
        }
    }
    impl<'a> From<&'a str> for Pin {
        fn from(s: &str) -> Self {
            let l = s.len().min(Pin::LEN);
            let chars = s.chars().take(l).chain(iter::repeat('0').take(Pin::LEN - l));
            Pin(
                chars
                    .map(|c| c.to_digit(16).unwrap_or(0))
                    .enumerate()
                    .fold(
                        0,
                        |pin, (i, d)| {
                            pin | ((d as u16) << (Pin::LEN - 1 - i) * Pin::LEN)
                        },
                    ),
            )
        }
    }
    impl<'a> From<Option<&'a str>> for Pin {
        fn from(p: Option<&'a str>) -> Self {
            p.unwrap_or("").into()
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
    impl ops::Deref for Pin {
        type Target = u16;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
}
