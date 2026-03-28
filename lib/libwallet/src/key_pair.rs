use core::fmt::Debug;
pub use derive::Derive;

type Bytes<const N: usize> = [u8; N];

/// A key pair with a public key
pub trait Pair: Signer + Derive {
    type Public: Public;

    fn from_bytes(seed: &[u8]) -> Option<Self>
    where
        Self: Sized;

    fn public(&self) -> Self::Public;
}

pub trait Public: AsRef<[u8]> + Debug {}
impl<const N: usize> Public for Bytes<N> {}

pub trait Signature: AsRef<[u8]> + Debug + PartialEq {}
impl<const N: usize> Signature for Bytes<N> {}

/// Something that can sign messages and identify itself.
pub trait Signer {
    type Signature: Signature;

    /// A human-readable identifier for this signer (e.g. "//Alice", "ledger-1").
    fn account_id(&self) -> &str;

    fn sign_msg(
        &self,
        data: impl AsRef<[u8]>,
    ) -> impl core::future::Future<Output = Result<Self::Signature, SigningError>>;

    fn verify(
        &self,
        msg: impl AsRef<[u8]>,
        sig: impl AsRef<[u8]>,
    ) -> impl core::future::Future<Output = bool>;
}

#[derive(Debug)]
pub enum SigningError {
    Locked,
    NoAccount,
}

impl core::fmt::Display for SigningError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            SigningError::Locked => write!(f, "Wallet is locked"),
            SigningError::NoAccount => write!(f, "No account available"),
        }
    }
}
/// Wrappers to represent any supported key pair.
pub mod any {
    use super::{Public, Signature, SigningError};
    use core::fmt;
    use zeroize::Zeroize;

    #[non_exhaustive]
    pub enum Pair {
        #[cfg(feature = "sr25519")]
        Sr25519(super::sr25519::Pair),
        #[cfg(feature = "secp256k1")]
        Secp256k1(super::secp256k1::Pair),
    }

    impl Drop for Pair {
        fn drop(&mut self) {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(ref mut kp) => kp.zeroize(),
                #[cfg(feature = "secp256k1")]
                Pair::Secp256k1(_) => {} // k256 zeroizes internally
            }
        }
    }

    impl fmt::Debug for Pair {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(_) => f.debug_tuple("Sr25519").field(&"<redacted>").finish(),
                #[cfg(feature = "secp256k1")]
                Pair::Secp256k1(_) => f.debug_tuple("Secp256k1").field(&"<redacted>").finish(),
            }
        }
    }

    impl super::Pair for Pair {
        type Public = AnyPublic;

        fn from_bytes(seed: &[u8]) -> Option<Self> {
            // Try sr25519 first, then secp256k1
            #[cfg(feature = "sr25519")]
            if let Some(p) = <super::sr25519::Pair as super::Pair>::from_bytes(seed) {
                return Some(Self::Sr25519(p));
            }
            #[cfg(feature = "secp256k1")]
            if let Some(p) = <super::secp256k1::Pair as super::Pair>::from_bytes(seed) {
                return Some(Self::Secp256k1(p));
            }
            None
        }

        fn public(&self) -> Self::Public {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(p) => AnyPublic::Sr25519(p.public()),
                #[cfg(feature = "secp256k1")]
                Pair::Secp256k1(p) => AnyPublic::Secp256k1(p.public()),
            }
        }
    }

    impl super::Derive for Pair {
        type Pair = Pair;

        fn derive(&self, path: &str) -> Self::Pair {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(kp) => Pair::Sr25519(kp.derive(path)),
                #[cfg(feature = "secp256k1")]
                Pair::Secp256k1(kp) => Pair::Secp256k1(kp.derive(path)),
            }
        }
    }

    #[cfg(feature = "sr25519")]
    impl From<super::sr25519::Pair> for Pair {
        fn from(p: super::sr25519::Pair) -> Self {
            Self::Sr25519(p)
        }
    }

    #[cfg(feature = "secp256k1")]
    impl From<super::secp256k1::Pair> for Pair {
        fn from(p: super::secp256k1::Pair) -> Self {
            Self::Secp256k1(p)
        }
    }

    impl super::Signer for Pair {
        type Signature = AnySignature;

        fn account_id(&self) -> &str {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(_) => "sr25519",
                #[cfg(feature = "secp256k1")]
                Pair::Secp256k1(_) => "secp256k1",
            }
        }

        async fn sign_msg(&self, msg: impl AsRef<[u8]>) -> Result<Self::Signature, SigningError> {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(p) => Ok(p.sign_msg(msg).await?.into()),
                #[cfg(feature = "secp256k1")]
                Pair::Secp256k1(p) => Ok(p.sign_msg(msg).await?.into()),
            }
        }

        async fn verify(&self, msg: impl AsRef<[u8]>, sig: impl AsRef<[u8]>) -> bool {
            match self {
                #[cfg(feature = "sr25519")]
                Pair::Sr25519(p) => super::Signer::verify(p, msg, sig).await,
                #[cfg(feature = "secp256k1")]
                Pair::Secp256k1(p) => super::Signer::verify(p, msg, sig).await,
            }
        }
    }

    #[derive(Debug)]
    #[non_exhaustive]
    pub enum AnyPublic {
        #[cfg(feature = "sr25519")]
        Sr25519(super::Bytes<{ super::sr25519::SEED_LEN }>),
        #[cfg(feature = "secp256k1")]
        Secp256k1(super::Bytes<33>),
    }

    impl AsRef<[u8]> for AnyPublic {
        fn as_ref(&self) -> &[u8] {
            match self {
                #[cfg(feature = "sr25519")]
                AnyPublic::Sr25519(p) => p.as_ref(),
                #[cfg(feature = "secp256k1")]
                AnyPublic::Secp256k1(p) => p.as_ref(),
            }
        }
    }

    impl fmt::Display for AnyPublic {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            for b in self.as_ref() {
                write!(f, "{:02X}", b)?;
            }
            Ok(())
        }
    }
    impl Public for AnyPublic {}

    #[derive(Debug, PartialEq)]
    #[non_exhaustive]
    pub enum AnySignature {
        #[cfg(feature = "sr25519")]
        Sr25519(super::Bytes<{ super::sr25519::SIG_LEN }>),
        #[cfg(feature = "secp256k1")]
        Secp256k1(super::Bytes<{ super::secp256k1::SIG_LEN }>),
    }

    impl AsRef<[u8]> for AnySignature {
        fn as_ref(&self) -> &[u8] {
            match self {
                #[cfg(feature = "sr25519")]
                AnySignature::Sr25519(s) => s.as_ref(),
                #[cfg(feature = "secp256k1")]
                AnySignature::Secp256k1(s) => s.as_ref(),
            }
        }
    }

    #[cfg(feature = "sr25519")]
    impl From<super::sr25519::Signature> for AnySignature {
        fn from(s: super::sr25519::Signature) -> Self {
            AnySignature::Sr25519(s)
        }
    }

    #[cfg(feature = "secp256k1")]
    impl From<super::secp256k1::Signature> for AnySignature {
        fn from(s: super::secp256k1::Signature) -> Self {
            AnySignature::Secp256k1(s)
        }
    }

    impl Signature for AnySignature {}
}

#[cfg(feature = "sr25519")]
pub mod sr25519 {
    use super::{derive::Junction, Bytes, Derive, Signer};
    use schnorrkel::{
        derive::{ChainCode, Derivation},
        signing_context, ExpansionMode, MiniSecretKey, SecretKey, MINI_SECRET_KEY_LENGTH,
    };

    pub use schnorrkel::Keypair as Pair;
    pub(super) const SEED_LEN: usize = MINI_SECRET_KEY_LENGTH;
    pub(super) const SIG_LEN: usize = 64;
    pub type Public = Bytes<32>;
    pub type Signature = Bytes<64>;
    const SIGNING_CTX: &[u8] = b"substrate";

    impl super::Pair for Pair {
        type Public = Public;

        fn from_bytes(bytes: &[u8]) -> Option<Self> {
            let minikey = MiniSecretKey::from_bytes(bytes.get(..SEED_LEN)?).ok()?;
            Some(minikey.expand_to_keypair(ExpansionMode::Ed25519))
        }

        fn public(&self) -> Self::Public {
            let mut key = [0u8; 32];
            key.copy_from_slice(self.public.as_ref());
            key
        }
    }

    impl Signer for Pair {
        type Signature = Signature;

        fn account_id(&self) -> &str {
            "sr25519"
        }

        async fn sign_msg(&self, msg: impl AsRef<[u8]>) -> Result<Self::Signature, super::SigningError> {
            let context = signing_context(SIGNING_CTX);
            Ok(self.sign(context.bytes(msg.as_ref())).to_bytes())
        }

        async fn verify(&self, msg: impl AsRef<[u8]>, sig: impl AsRef<[u8]>) -> bool {
            let sig = match schnorrkel::Signature::from_bytes(sig.as_ref()) {
                Ok(s) => s,
                Err(_) => return false,
            };
            self.public
                .verify_simple(SIGNING_CTX, msg.as_ref(), &sig)
                .is_ok()
        }
    }

    impl Derive for Pair {
        type Pair = Self;

        fn derive(&self, path: &str) -> Self
        where
            Self: Sized,
        {
            super::derive::parse_substrate_junctions(path)
                .fold(self.secret.clone(), |key, (part, hard)| {
                    if hard {
                        key.hard_derive_mini_secret_key(Some(ChainCode(part)), [])
                            .0
                            .expand(ExpansionMode::Ed25519)
                    } else {
                        derive_simple(key, part)
                    }
                })
                .into()
        }
    }

    fn derive_simple(key: SecretKey, j: Junction) -> SecretKey {
        use rand_chacha::rand_core::SeedableRng;
        // Seed from secret key bytes for a per-key deterministic nonce
        // instead of a globally predictable zero seed.
        let key_bytes = key.to_bytes();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&key_bytes[..32]);
        let rng = rand_chacha::ChaChaRng::from_seed(seed);
        key.derived_key_simple_rng(ChainCode(j), &[], rng).0
    }

    #[cfg(test)]
    mod tests {
        use crate::{Derive, Pair};

        #[test]
        fn derive_substrate_keypair() {
            // "rotate increase color sustain print future moon rigid hunt wild diagram online"
            let seed = b"\x70\x8a\x2b\xe9\x96\xb8\x7d\x1e\x7b\xb2\x3f\x3c\xfa\x9b\xac\x80\x4c\x83\x35\x9e\x30\x85\x98\xb0\xcb\x20\x72\x82\x90\x68\x47\x57";

            for (path, pubkey) in [
                // from subkey
                (
                    "//test",
                    b"\x0a\x04\x17\x5e\x09\x7c\x49\x26\x45\xa9\x8e\x1f\x28\x18\xa3\x95\x07\xb9\xfc\xba\x02\x03\x4d\x24\x4d\x27\xa3\x4d\xd3\xea\x2a\x11",
                ), (
                    "/test",
                    b"\x9e\x75\x15\xf2\x87\x0a\xee\x0c\x54\x5f\x84\x35\x1f\xd4\xed\xd3\xc2\x48\x26\x8d\x2c\xb5\xfd\x97\x88\x55\x12\x10\xb8\x99\x9b\x76",
                ), (
                    "//test//123",
                    b"\x50\xb3\x99\x79\xff\x3b\x54\x7d\x41\x7c\x8e\xda\xe8\xab\x84\x21\x0a\x6d\xef\x64\x14\x3f\x3e\xdc\x46\x7a\xf5\x2a\xf5\x53\x72\x06",
                ), (
                    "//test/123",
                    b"\x7a\x39\xc7\x6b\x2a\x0c\x25\xc7\x37\x92\x0d\x5a\x4c\xc4\x07\x6e\xdd\x7a\xe2\xf0\x48\x99\x9b\x92\x54\xa7\xe6\x11\xcf\xf8\x78\x3a",
                ), (
                    "/test/123",
                    b"\x48\xce\x4b\x7e\x7c\xe5\x87\xf6\xad\x1e\x14\x96\x51\x77\x94\xf1\x28\x82\xb9\xff\x69\xc9\x11\xf7\xda\x7c\x15\x7a\xdc\x9d\x24\x4e",
                ),
            ] {
                let root: super::Pair = Pair::from_bytes(seed).unwrap();
                let derived = root.derive(path);
                assert_eq!(&derived.public(), pubkey);
            }
        }

        #[test]
        #[cfg(all(feature = "substrate", feature = "mnemonic"))]
        fn derive_keypair_from_phrase() {
            use mnemonic::Mnemonic;
            let phrase =
                "rotate increase color sustain print future moon rigid hunt wild diagram online";

            for (path, pubkey) in [
                (
                    "//test",
                    b"\x0a\x04\x17\x5e\x09\x7c\x49\x26\x45\xa9\x8e\x1f\x28\x18\xa3\x95\x07\xb9\xfc\xba\x02\x03\x4d\x24\x4d\x27\xa3\x4d\xd3\xea\x2a\x11",
                ), (
                    "/test",
                    b"\x9e\x75\x15\xf2\x87\x0a\xee\x0c\x54\x5f\x84\x35\x1f\xd4\xed\xd3\xc2\x48\x26\x8d\x2c\xb5\xfd\x97\x88\x55\x12\x10\xb8\x99\x9b\x76",
                ), (
                    "//test//123",
                    b"\x50\xb3\x99\x79\xff\x3b\x54\x7d\x41\x7c\x8e\xda\xe8\xab\x84\x21\x0a\x6d\xef\x64\x14\x3f\x3e\xdc\x46\x7a\xf5\x2a\xf5\x53\x72\x06",
                ), (
                    "//test/123",
                    b"\x7a\x39\xc7\x6b\x2a\x0c\x25\xc7\x37\x92\x0d\x5a\x4c\xc4\x07\x6e\xdd\x7a\xe2\xf0\x48\x99\x9b\x92\x54\xa7\xe6\x11\xcf\xf8\x78\x3a",
                ), (
                    "/test/123",
                    b"\x48\xce\x4b\x7e\x7c\xe5\x87\xf6\xad\x1e\x14\x96\x51\x77\x94\xf1\x28\x82\xb9\xff\x69\xc9\x11\xf7\xda\x7c\x15\x7a\xdc\x9d\x24\x4e",
                ),
            ] {
                let phrase = Mnemonic::from_phrase(phrase).unwrap();
                let seed = crate::substrate_seed(phrase.entropy(), "");

                let root: super::Pair = Pair::from_bytes(&*seed).expect("valid seed");
                let derived = root.derive(path);
                assert_eq!(&derived.public(), pubkey);
            }
        }
    }
}

#[cfg(feature = "secp256k1")]
pub mod secp256k1 {
    use super::{Bytes, Signer};
    use k256::ecdsa::{self, signature::Verifier};

    pub const SEED_LEN: usize = 32;
    pub const SIG_LEN: usize = 65; // r(32) + s(32) + recovery_id(1)
    pub type Public = Bytes<33>; // compressed public key
    pub type Signature = Bytes<SIG_LEN>;

    pub struct Pair {
        secret: k256::ecdsa::SigningKey,
    }

    impl super::Pair for Pair {
        type Public = Public;

        fn from_bytes(bytes: &[u8]) -> Option<Self> {
            let secret = ecdsa::SigningKey::from_slice(bytes.get(..SEED_LEN)?).ok()?;
            Some(Pair { secret })
        }

        fn public(&self) -> Self::Public {
            let vk = self.secret.verifying_key();
            let compressed = vk.to_encoded_point(true);
            let mut key = [0u8; 33];
            key.copy_from_slice(compressed.as_bytes());
            key
        }
    }

    impl Signer for Pair {
        type Signature = Signature;

        fn account_id(&self) -> &str {
            "secp256k1"
        }

        async fn sign_msg(&self, msg: impl AsRef<[u8]>) -> Result<Self::Signature, super::SigningError> {
            let (sig, recid) = self.secret
                .sign_prehash_recoverable(msg.as_ref())
                .map_err(|_| super::SigningError::Locked)?;
            let mut out = [0u8; SIG_LEN];
            out[..64].copy_from_slice(&sig.to_bytes());
            out[64] = recid.to_byte();
            Ok(out)
        }

        async fn verify(&self, msg: impl AsRef<[u8]>, sig: impl AsRef<[u8]>) -> bool {
            let sig_bytes = sig.as_ref();
            if sig_bytes.len() < 64 {
                return false;
            }
            let Ok(sig) = ecdsa::Signature::from_slice(&sig_bytes[..64]) else {
                return false;
            };
            self.secret.verifying_key().verify(msg.as_ref(), &sig).is_ok()
        }
    }

    impl super::Derive for Pair {
        type Pair = Self;

        fn derive(&self, _path: &str) -> Self {
            // BIP32 derivation is handled by the chain-specific vault wrapper,
            // not by the raw key pair. This is a no-op placeholder.
            Pair {
                secret: self.secret.clone(),
            }
        }
    }

    impl Drop for Pair {
        fn drop(&mut self) {
            // k256::SigningKey zeroizes on drop internally
        }
    }

    impl core::fmt::Debug for Pair {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("secp256k1::Pair").field("key", &"<redacted>").finish()
        }
    }
}

mod derive {
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
        path.split_inclusive('/')
            .flat_map(|s| if s == "/" { "" } else { s }.split('/')) // "//Alice//Bob" -> ["","","Alice","","","Bob"]
            .scan(0u8, |j, part| {
                Some(if part.is_empty() {
                    *j += 1;
                    None
                } else {
                    let hard = *j > 1;
                    *j = 0;
                    Some((encoded_junction(part), hard))
                })
            })
            .flatten()
    }

    /// Encode a junction name into a fixed-size byte array.
    /// Names longer than 31 bytes are silently truncated, matching Substrate behavior.
    fn encoded_junction(part: &str) -> Junction {
        let mut code = [0; JUNCTION_LEN];
        if let Ok(n) = part.parse::<u64>() {
            code[..8].copy_from_slice(&n.to_le_bytes());
        } else {
            let len = part.len().min(JUNCTION_LEN - 1);
            code[0] = (len as u8) << 2;
            code[1..len + 1].copy_from_slice(part[..len].as_bytes());
        }
        code
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        extern crate alloc;
        use alloc::vec::Vec;

        #[test]
        fn substrate_junctions() {
            let path = "//Alice//Bob/123//loremipsumdolor";
            let out: Vec<_> = parse_substrate_junctions(path).map(|(_, h)| h).collect();
            assert_eq!(out, vec![true, true, false, true]);
        }
    }
}
