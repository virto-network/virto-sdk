//! Wallet and session signers backed by the OS secure store.

use anyhow::Result;
use libwallet::KeyStore;
use libwallet::vault::{OSKeyring, Vault, VaultError};

use crate::profiles;

/// The `//default` sr25519 key of a stored secret.
pub type WalletSigner = sube::libwallet::LibwalletSigner<libwallet::vault::utils::DerivedSigner>;

/// Derive the `//default` sr25519 signer from `keys`.
async fn unlock<K: KeyStore>(keys: K) -> Result<WalletSigner>
where
    VaultError<K::Error>: std::error::Error + Send + Sync + 'static,
{
    let derived = libwallet::Substrate::new(keys).unlock(None, ()).await?;
    let account: [u8; 32] = derived
        .public()
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signer is not AccountId32"))?;
    Ok(WalletSigner::new(
        derived,
        account,
        sube::libwallet::SignatureScheme::Sr25519,
    ))
}

pub async fn wallet_signer(profile: &profiles::WalletProfile) -> Result<WalletSigner> {
    if !profile.scheme.eq_ignore_ascii_case("sr25519") {
        anyhow::bail!("unsupported wallet profile scheme {}", profile.scheme);
    }
    let signer = unlock(OSKeyring::<()>::new(&profile.secure_entry, None)).await?;
    if sube::Signer::account(&signer) != profile.account {
        anyhow::bail!("wallet secure entry does not match the profile account");
    }
    Ok(signer)
}

/// A fresh session signer and the seed to store once it is registered.
#[cfg(feature = "pass")]
pub async fn generate_session_signer() -> Result<(WalletSigner, [u8; 32])> {
    let mut keys = libwallet::vault::Simple::<(), 32>::generate(&mut rand_core::OsRng);
    let seed = *keys.unlock()?;
    Ok((unlock(keys).await?, seed))
}

#[cfg(feature = "pass")]
pub async fn load_session_signer(secure_entry: &str, expected: [u8; 32]) -> Result<WalletSigner> {
    let signer = unlock(OSKeyring::<()>::new(secure_entry, None)).await?;
    if sube::Signer::account(&signer) != expected {
        anyhow::bail!("stored session key does not match the pass profile");
    }
    Ok(signer)
}
