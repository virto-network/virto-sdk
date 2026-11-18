//! Wallet and session signers backed by the OS secure store.

use anyhow::Result;
use libwallet::KeyStore;
use libwallet::vault::{OSKeyring, VaultError};

use crate::profiles;

/// An sr25519 key of a stored secret.
pub type WalletSigner = sube::libwallet::LibwalletSigner<libwallet::any::Pair>;

/// The sr25519 `key` of a mnemonic's entropy, and its account.
pub fn wallet_key(
    entropy: &[u8],
    key: profiles::WalletKey,
) -> Result<(libwallet::any::Pair, [u8; 32])> {
    use libwallet::Pair;

    let seed = libwallet::seed_from_entropy(entropy, "");
    let pair = match key {
        profiles::WalletKey::Root => libwallet::any::Pair::from_bytes(&*seed),
        profiles::WalletKey::Default => libwallet::vault::utils::RootAccount::from_bytes(&*seed)
            .map(|root| root.derive("//default")),
    }
    .ok_or_else(|| anyhow::anyhow!("could not derive the wallet key"))?;
    let account = pair
        .public()
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signer is not AccountId32"))?;
    Ok((pair, account))
}

/// The sr25519 `key` signer of the secret in `keys`.
fn unlock<K: KeyStore>(mut keys: K, key: profiles::WalletKey) -> Result<WalletSigner>
where
    VaultError<K::Error>: std::error::Error + Send + Sync + 'static,
{
    let entropy = keys.unlock().map_err(VaultError::KeyStore)?;
    let (pair, account) = wallet_key(entropy, key)?;
    Ok(WalletSigner::new(
        pair,
        account,
        sube::libwallet::SignatureScheme::Sr25519,
    ))
}

pub async fn wallet_signer(profile: &profiles::WalletProfile) -> Result<WalletSigner> {
    let signer = unlock(
        OSKeyring::<()>::new(&profile.secure_entry(), None),
        profile.key,
    )?;
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
    // Session keys are internal, and stay at `//default`.
    Ok((unlock(keys, profiles::WalletKey::Default)?, seed))
}

#[cfg(feature = "pass")]
pub async fn load_session_signer(secure_entry: &str, expected: [u8; 32]) -> Result<WalletSigner> {
    let signer = unlock(
        OSKeyring::<()>::new(secure_entry, None),
        profiles::WalletKey::Default,
    )?;
    if sube::Signer::account(&signer) != expected {
        anyhow::bail!("stored session key does not match the pass profile");
    }
    Ok(signer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Substrate's development phrase, whose root key `subkey inspect`
    /// reports as 0x46eb…b47a.
    #[test]
    fn root_keys_match_what_wallets_show() {
        let mnemonic: libwallet::Mnemonic =
            "bottom drive obey lake curtain smoke basket hold race lonely fit walk"
                .parse()
                .unwrap();
        let (_, root) = wallet_key(mnemonic.entropy(), profiles::WalletKey::Root).unwrap();
        assert_eq!(
            hex::encode(root),
            "46ebddef8cd9bb167dc30878d7113b7e168e6f0646beffd77d69d39bad76b47a"
        );
        let (_, default) = wallet_key(mnemonic.entropy(), profiles::WalletKey::Default).unwrap();
        assert_ne!(default, root);
    }
}
