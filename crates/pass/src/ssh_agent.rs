//! Ed25519 SSH-agent device provider with no private-key export.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use codec::Encode;
use sha2::{Digest, Sha256};

use crate::workflow::{
    AssertionRequest, AttestationRequest, DeviceAttestation, DeviceAuthenticator,
};
use crate::{DeviceId, blake2b_256};
use sube::{DynValue, Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentKey {
    /// OpenSSH SHA256 fingerprint selected by the user.
    pub fingerprint: String,
    /// SSH wire-format public key blob.
    pub public_key: Vec<u8>,
    pub algorithm: String,
}

pub trait SshAgentTransport {
    async fn keys(&self) -> core::result::Result<Vec<AgentKey>, String>;
    async fn sign(
        &self,
        public_key: &[u8],
        message: &[u8],
    ) -> core::result::Result<Vec<u8>, String>;
}

pub struct SshAgentDevice<T> {
    transport: T,
    fingerprint: String,
    namespace: String,
}

impl<T> SshAgentDevice<T> {
    pub fn new(transport: T, fingerprint: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            transport,
            fingerprint: fingerprint.into(),
            namespace: namespace.into(),
        }
    }
}

impl<T: SshAgentTransport> SshAgentDevice<T> {
    async fn selected_key(&self) -> Result<AgentKey> {
        let keys = self
            .transport
            .keys()
            .await
            .map_err(|error| Error::Signing(format!("ssh-agent: {error}")))?;
        let key = keys
            .into_iter()
            .find(|key| key.fingerprint == self.fingerprint)
            .ok_or_else(|| Error::Signing("ssh-agent fingerprint not found".into()))?;
        if key.algorithm != "ssh-ed25519" {
            return Err(Error::Signing(
                "selected ssh-agent key is not Ed25519".into(),
            ));
        }
        Ok(key)
    }

    async fn sign_challenge(
        &self,
        key: &AgentKey,
        context: u32,
        challenge: [u8; 32],
        authority: [u8; 32],
    ) -> Result<Vec<u8>> {
        let message = (context, challenge, authority).encode();
        let signed = sshsig_signed_data(&self.namespace, &message);
        self.transport
            .sign(&key.public_key, &signed)
            .await
            .map_err(|error| Error::Signing(format!("ssh-agent: {error}")))
    }
}

impl<T: SshAgentTransport> DeviceAuthenticator for SshAgentDevice<T> {
    async fn attest(&self, request: &AttestationRequest) -> Result<DeviceAttestation> {
        let key = self.selected_key().await?;
        let signature = self
            .sign_challenge(
                &key,
                request.context,
                request.challenge,
                request.authority_id.0,
            )
            .await?;
        let device_id = DeviceId(blake2b_256(&key.public_key));
        Ok(DeviceAttestation {
            device_id,
            variant: "Ssh".into(),
            payload: DynValue::obj(&[
                (
                    "meta",
                    DynValue::obj(&[
                        ("authority_id", DynValue::from(request.authority_id.0)),
                        ("device_id", DynValue::from(device_id.0)),
                        ("context", DynValue::from(request.context)),
                    ]),
                ),
                ("public_key", DynValue::from(key.public_key)),
                ("signature", DynValue::from(signature)),
            ]),
        })
    }

    async fn assert(&self, request: &AssertionRequest) -> Result<DynValue> {
        let key = self.selected_key().await?;
        let signature = self
            .sign_challenge(
                &key,
                request.context,
                request.challenge,
                request.authority_id.0,
            )
            .await?;
        Ok(DynValue::obj(&[(
            "Ssh",
            DynValue::obj(&[
                ("user_id", DynValue::from(request.user_id.0)),
                (
                    "message",
                    DynValue::obj(&[
                        ("context", DynValue::from(request.context)),
                        ("challenge", DynValue::from(request.challenge)),
                        ("authority_id", DynValue::from(request.authority_id.0)),
                    ]),
                ),
                ("public_key", DynValue::from(key.public_key)),
                ("signature", DynValue::from(signature)),
            ]),
        )]))
    }
}

fn ssh_string(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u32).to_be_bytes());
    output.extend_from_slice(value);
}

/// Data signed by an SSHSIG signer, following PROTOCOL.sshsig.
pub fn sshsig_signed_data(namespace: &str, message: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(message);
    let mut output = Vec::new();
    output.extend_from_slice(b"SSHSIG");
    ssh_string(&mut output, namespace.as_bytes());
    ssh_string(&mut output, b"");
    ssh_string(&mut output, b"sha256");
    ssh_string(&mut output, &digest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use core::cell::RefCell;

    struct MockAgent {
        signed: RefCell<Vec<u8>>,
    }

    impl SshAgentTransport for MockAgent {
        async fn keys(&self) -> core::result::Result<Vec<AgentKey>, String> {
            Ok(vec![AgentKey {
                fingerprint: "SHA256:test".into(),
                public_key: vec![7; 32],
                algorithm: "ssh-ed25519".into(),
            }])
        }

        async fn sign(&self, _: &[u8], message: &[u8]) -> core::result::Result<Vec<u8>, String> {
            *self.signed.borrow_mut() = message.to_vec();
            Ok(vec![9; 64])
        }
    }

    #[test]
    fn emits_sshsig_compatible_agent_message() {
        let agent = MockAgent {
            signed: RefCell::new(Vec::new()),
        };
        let device = SshAgentDevice::new(agent, "SHA256:test", "pallet-pass");
        let request = AssertionRequest {
            user_id: crate::HashedUserId([1; 32]),
            authority_id: crate::AuthorityId([2; 32]),
            context: 3,
            block_hash: [4; 32],
            binding: [5; 32],
            challenge: [6; 32],
        };
        futures_lite::future::block_on(device.assert(&request)).unwrap();
        let signed = device.transport.signed.borrow();
        assert!(signed.starts_with(b"SSHSIG"));
        assert!(signed.windows(11).any(|window| window == b"pallet-pass"));
    }
}
