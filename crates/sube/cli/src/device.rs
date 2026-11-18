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
    WebAuthn(pass::webauthn::WebAuthnDevice<WebAuthnTransport>),
}

#[cfg(all(feature = "desktop-webauthn", not(feature = "soft-webauthn")))]
type WebAuthnTransport = pass::webauthn::DesktopWebAuthnTransport;
#[cfg(feature = "soft-webauthn")]
type WebAuthnTransport = pass::webauthn::SoftWebAuthnTransport;

#[cfg(feature = "desktop-webauthn")]
fn webauthn_transport() -> WebAuthnTransport {
    #[cfg(feature = "soft-webauthn")]
    {
        // One authenticator per thread, so that a credential created at
        // enrollment can sign later in the same process.
        thread_local!(static SOFT: WebAuthnTransport = WebAuthnTransport::default());
        SOFT.with(Clone::clone)
    }
    #[cfg(not(feature = "soft-webauthn"))]
    pass::webauthn::DesktopWebAuthnTransport
}

#[cfg(feature = "desktop-webauthn")]
/// Authenticator prompts, such as touching a security key or entering its
/// PIN, happen on the terminal, outside of the CLI's control.
pub enum Prompt {
    Begin,
    End,
}

#[cfg(feature = "desktop-webauthn")]
static PROMPT_HOOK: std::sync::OnceLock<Box<dyn Fn(Prompt) + Send + Sync>> =
    std::sync::OnceLock::new();

#[cfg(feature = "desktop-webauthn")]
/// Run `hook` around authenticator prompts, so that a full-screen UI can
/// give the terminal to them. `Prompt::Begin` must return only once it has.
pub fn on_prompt(hook: impl Fn(Prompt) + Send + Sync + 'static) {
    let _ = PROMPT_HOOK.set(Box::new(hook));
}

#[cfg(feature = "desktop-webauthn")]
/// Marks an authenticator prompt for as long as it lives.
struct Prompting;

#[cfg(feature = "desktop-webauthn")]
impl Prompting {
    fn begin() -> Self {
        if let Some(hook) = PROMPT_HOOK.get() {
            hook(Prompt::Begin);
        }
        Prompting
    }
}

#[cfg(feature = "desktop-webauthn")]
impl Drop for Prompting {
    fn drop(&mut self) {
        if let Some(hook) = PROMPT_HOOK.get() {
            hook(Prompt::End);
        }
    }
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
                use pass::webauthn::{WebAuthnDevice, WebAuthnProfile};
                Provider::WebAuthn(if credential_id.is_empty() {
                    WebAuthnDevice::for_enrollment(webauthn_transport(), rp_id, origin)
                } else {
                    WebAuthnDevice::from_profile(
                        webauthn_transport(),
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
        }
    }

    async fn attest(&self, request: &ChallengeRequest) -> sube::Result<DeviceAttestation> {
        match &self.provider {
            Provider::Wallet(signer) => wallet_device(signer).attest(request).await,
            #[cfg(feature = "desktop-webauthn")]
            Provider::WebAuthn(device) => {
                let _prompting = Prompting::begin();
                device.attest(request).await
            }
        }
    }

    async fn assert(&self, request: &ChallengeRequest) -> sube::Result<DynValue> {
        match &self.provider {
            Provider::Wallet(signer) => wallet_device(signer).assert(request).await,
            #[cfg(feature = "desktop-webauthn")]
            Provider::WebAuthn(device) => {
                let _prompting = Prompting::begin();
                device.assert(request).await
            }
        }
    }
}
