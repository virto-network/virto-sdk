//! Wallet and session signers backed by the OS secure store.

use anyhow::Result;

use crate::profiles;

#[cfg(feature = "wallet")]
pub async fn wallet_signer(
    profile: &profiles::WalletProfile,
) -> Result<sube::libwallet::LibwalletSigner<libwallet::vault::utils::DerivedSigner>> {
    use libwallet::vault::Vault;

    if !profile.scheme.eq_ignore_ascii_case("sr25519") {
        anyhow::bail!("unsupported wallet profile scheme {}", profile.scheme);
    }
    let keys = libwallet::vault::OSKeyring::<()>::new(&profile.secure_entry, None);
    let mut vault = libwallet::Substrate::new(keys);
    let signer = vault.unlock(None, ()).await?;
    let public = signer.public();
    if public.as_ref() != profile.account {
        anyhow::bail!("wallet secure entry does not match the profile account");
    }
    Ok(sube::libwallet::LibwalletSigner::new(
        signer,
        profile.account,
        sube::libwallet::SignatureScheme::Sr25519,
    ))
}

#[cfg(feature = "pass")]
pub async fn generate_session_signer() -> Result<(
    sube::libwallet::LibwalletSigner<libwallet::vault::utils::DerivedSigner>,
    [u8; 32],
)> {
    use libwallet::vault::Vault;

    let mut keys = libwallet::vault::Simple::<(), 32>::generate(&mut rand_core::OsRng);
    let seed = *keys.unlock()?;
    let mut vault = libwallet::Substrate::new(keys);
    let derived = vault.unlock(None, ()).await?;
    let public = derived.public();
    let account: [u8; 32] = public
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("session signer is not AccountId32"))?;
    Ok((
        sube::libwallet::LibwalletSigner::new(
            derived,
            account,
            sube::libwallet::SignatureScheme::Sr25519,
        ),
        seed,
    ))
}

#[cfg(feature = "pass")]
pub async fn load_session_signer(
    secure_entry: &str,
    expected: Option<[u8; 32]>,
) -> Result<sube::libwallet::LibwalletSigner<libwallet::vault::utils::DerivedSigner>> {
    use libwallet::vault::Vault;

    let keys = libwallet::vault::OSKeyring::<()>::new(secure_entry, None);
    let mut vault = libwallet::Substrate::new(keys);
    let derived = vault.unlock(None, ()).await?;
    let public = derived.public();
    let account: [u8; 32] = public
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("session signer is not AccountId32"))?;
    if expected.is_some_and(|expected| expected != account) {
        anyhow::bail!("stored session key does not match the pass profile");
    }
    Ok(sube::libwallet::LibwalletSigner::new(
        derived,
        account,
        sube::libwallet::SignatureScheme::Sr25519,
    ))
}
