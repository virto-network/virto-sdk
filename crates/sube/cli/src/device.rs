//! The authenticator behind a stored pass device, whatever its provider.

use anyhow::Result;
use pass::DeviceAuthenticator;
use pass::workflow::{ChallengeRequest, DeviceAttestation};
use sube::DynValue;

use crate::profiles::{DeviceProviderProfile, Profiles};
use crate::signers::WalletSigner;

/// A device opened from its stored profile. Provider support is selected
/// at build time here and nowhere else.
pub struct Device {
    stored: DeviceProviderProfile,
    provider: Provider,
}

// Only one short-lived device exists at a time.
#[allow(clippy::large_enum_variant)]
enum Provider {
    Wallet(WalletSigner),
    #[cfg(feature = "desktop-webauthn")]
    WebAuthn(pass::webauthn::WebAuthnDevice<pass::webauthn::DesktopWebAuthnTransport>),
    #[cfg(all(feature = "ssh-agent", unix))]
    Ssh(pass::ssh_agent::SshAgentDevice<pass::ssh_agent::UnixSshAgent>),
}

impl Device {
    /// Open the device `stored` describes. A substrate-key device is a
    /// wallet profile of the chain `genesis_hash`; a WebAuthn device without
    /// a credential id creates its credential when it attests.
    pub async fn open(
        profiles: &Profiles,
        genesis_hash: [u8; 32],
        stored: &DeviceProviderProfile,
    ) -> Result<Self> {
        let provider = match stored {
            DeviceProviderProfile::SubstrateKey { wallet } => {
                let profile = crate::workflow::wallet_profile(profiles, wallet, genesis_hash)?;
                Provider::Wallet(crate::signers::wallet_signer(profile).await?)
            }
            #[cfg(feature = "desktop-webauthn")]
            DeviceProviderProfile::WebAuthn {
                rp_id,
                origin,
                credential_id,
            } => {
                use pass::webauthn::{DesktopWebAuthnTransport, WebAuthnDevice, WebAuthnProfile};
                Provider::WebAuthn(if credential_id.is_empty() {
                    WebAuthnDevice::for_enrollment(DesktopWebAuthnTransport, rp_id, origin)
                } else {
                    WebAuthnDevice::from_profile(
                        DesktopWebAuthnTransport,
                        WebAuthnProfile {
                            rp_id: rp_id.clone(),
                            origin: origin.clone(),
                            credential_id: credential_id.clone(),
                        },
                    )
                })
            }
            #[cfg(not(feature = "desktop-webauthn"))]
            DeviceProviderProfile::WebAuthn { .. } => {
                anyhow::bail!("this build has no desktop WebAuthn support")
            }
            #[cfg(all(feature = "ssh-agent", unix))]
            DeviceProviderProfile::SshAgent {
                fingerprint,
                namespace,
            } => {
                let transport =
                    pass::ssh_agent::UnixSshAgent::from_env().map_err(anyhow::Error::msg)?;
                Provider::Ssh(pass::ssh_agent::SshAgentDevice::new(
                    transport,
                    fingerprint,
                    namespace,
                ))
            }
            #[cfg(not(all(feature = "ssh-agent", unix)))]
            DeviceProviderProfile::SshAgent { .. } => {
                anyhow::bail!("this build has no native SSH-agent support")
            }
        };
        Ok(Self {
            stored: stored.clone(),
            provider,
        })
    }

    /// What to record for this device, including the credential a new
    /// WebAuthn device created when it attested.
    pub fn stored_profile(&self) -> Result<DeviceProviderProfile> {
        match &self.provider {
            #[cfg(feature = "desktop-webauthn")]
            Provider::WebAuthn(device) => {
                let profile = device.profile().ok_or_else(|| {
                    anyhow::anyhow!("WebAuthn authenticator returned no credential id")
                })?;
                Ok(DeviceProviderProfile::WebAuthn {
                    rp_id: profile.rp_id,
                    origin: profile.origin,
                    credential_id: profile.credential_id,
                })
            }
            _ => Ok(self.stored.clone()),
        }
    }
}

fn wallet_device(signer: &WalletSigner) -> pass::wallet::WalletDevice<'_, impl libwallet::Signer> {
    pass::wallet::WalletDevice::new(
        signer.inner(),
        sube::Signer::account(signer),
        pass::wallet::SignatureType::Sr25519,
    )
}

impl DeviceAuthenticator for Device {
    fn variant(&self) -> &'static str {
        match &self.provider {
            Provider::Wallet(signer) => wallet_device(signer).variant(),
            #[cfg(feature = "desktop-webauthn")]
            Provider::WebAuthn(device) => device.variant(),
            #[cfg(all(feature = "ssh-agent", unix))]
            Provider::Ssh(device) => device.variant(),
        }
    }

    async fn attest(&self, request: &ChallengeRequest) -> sube::Result<DeviceAttestation> {
        match &self.provider {
            Provider::Wallet(signer) => wallet_device(signer).attest(request).await,
            #[cfg(feature = "desktop-webauthn")]
            Provider::WebAuthn(device) => device.attest(request).await,
            #[cfg(all(feature = "ssh-agent", unix))]
            Provider::Ssh(device) => device.attest(request).await,
        }
    }

    async fn assert(&self, request: &ChallengeRequest) -> sube::Result<DynValue> {
        match &self.provider {
            Provider::Wallet(signer) => wallet_device(signer).assert(request).await,
            #[cfg(feature = "desktop-webauthn")]
            Provider::WebAuthn(device) => device.assert(request).await,
            #[cfg(all(feature = "ssh-agent", unix))]
            Provider::Ssh(device) => device.assert(request).await,
        }
    }
}
