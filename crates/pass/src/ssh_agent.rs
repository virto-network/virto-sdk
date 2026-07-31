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

#[cfg(all(feature = "unix-ssh-agent", unix))]
mod unix {
    use alloc::format;
    use alloc::string::{String, ToString};
    use alloc::vec;
    use alloc::vec::Vec;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};

    use sha2::{Digest, Sha256};

    use super::{AgentKey, SshAgentTransport};

    const REQUEST_IDENTITIES: u8 = 11;
    const IDENTITIES_ANSWER: u8 = 12;
    const SIGN_REQUEST: u8 = 13;
    const SIGN_RESPONSE: u8 = 14;
    const MAX_AGENT_FRAME: usize = 1024 * 1024;

    /// Native Unix transport for the OpenSSH agent protocol.
    ///
    /// Every operation creates a short-lived connection to the socket. Private
    /// key material remains inside the agent.
    #[derive(Clone, Debug)]
    pub struct UnixSshAgent {
        socket: PathBuf,
    }

    impl UnixSshAgent {
        pub fn new(socket: impl Into<PathBuf>) -> Self {
            Self {
                socket: socket.into(),
            }
        }

        pub fn from_env() -> core::result::Result<Self, String> {
            std::env::var_os("SSH_AUTH_SOCK")
                .map(PathBuf::from)
                .map(Self::new)
                .ok_or_else(|| "SSH_AUTH_SOCK is not set".into())
        }

        pub fn socket(&self) -> &Path {
            &self.socket
        }

        fn request(&self, payload: &[u8]) -> core::result::Result<Vec<u8>, String> {
            let mut stream = UnixStream::connect(&self.socket)
                .map_err(|error| format!("connect {}: {error}", self.socket.display()))?;
            let length = u32::try_from(payload.len())
                .map_err(|_| "ssh-agent request is too large".to_string())?;
            stream
                .write_all(&length.to_be_bytes())
                .and_then(|_| stream.write_all(payload))
                .map_err(|error| format!("write ssh-agent request: {error}"))?;

            let mut encoded_length = [0u8; 4];
            stream
                .read_exact(&mut encoded_length)
                .map_err(|error| format!("read ssh-agent response length: {error}"))?;
            let length = u32::from_be_bytes(encoded_length) as usize;
            if length == 0 || length > MAX_AGENT_FRAME {
                return Err("ssh-agent response has an invalid length".into());
            }
            let mut response = vec![0u8; length];
            stream
                .read_exact(&mut response)
                .map_err(|error| format!("read ssh-agent response: {error}"))?;
            Ok(response)
        }
    }

    impl SshAgentTransport for UnixSshAgent {
        async fn keys(&self) -> core::result::Result<Vec<AgentKey>, String> {
            let response = self.request(&[REQUEST_IDENTITIES])?;
            let mut cursor = response.as_slice();
            expect_message(&mut cursor, IDENTITIES_ANSWER)?;
            let count = take_u32(&mut cursor)?;
            if count > 1024 {
                return Err("ssh-agent returned too many identities".into());
            }
            let mut keys = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let public_key = take_string(&mut cursor)?.to_vec();
                let _comment = take_string(&mut cursor)?;
                let mut key_cursor = public_key.as_slice();
                let algorithm = core::str::from_utf8(take_string(&mut key_cursor)?)
                    .map_err(|_| "ssh-agent key algorithm is not UTF-8")?
                    .to_string();
                keys.push(AgentKey {
                    fingerprint: openssh_fingerprint(&public_key),
                    public_key,
                    algorithm,
                });
            }
            if !cursor.is_empty() {
                return Err("ssh-agent identity response has trailing bytes".into());
            }
            Ok(keys)
        }

        async fn sign(
            &self,
            public_key: &[u8],
            message: &[u8],
        ) -> core::result::Result<Vec<u8>, String> {
            let mut request = vec![SIGN_REQUEST];
            put_string(&mut request, public_key)?;
            put_string(&mut request, message)?;
            request.extend_from_slice(&0u32.to_be_bytes());

            let response = self.request(&request)?;
            let mut cursor = response.as_slice();
            expect_message(&mut cursor, SIGN_RESPONSE)?;
            let signature_blob = take_string(&mut cursor)?;
            if !cursor.is_empty() {
                return Err("ssh-agent signature response has trailing bytes".into());
            }
            let mut signature_cursor = signature_blob;
            let algorithm = take_string(&mut signature_cursor)?;
            if algorithm != b"ssh-ed25519" {
                return Err("ssh-agent returned a non-Ed25519 signature".into());
            }
            let signature = take_string(&mut signature_cursor)?;
            if signature.len() != 64 || !signature_cursor.is_empty() {
                return Err("ssh-agent returned an invalid Ed25519 signature".into());
            }
            Ok(signature.to_vec())
        }
    }

    fn expect_message(cursor: &mut &[u8], expected: u8) -> core::result::Result<(), String> {
        let Some((message, rest)) = cursor.split_first() else {
            return Err("empty ssh-agent response".into());
        };
        if *message != expected {
            return Err(format!("unexpected ssh-agent response type {message}"));
        }
        *cursor = rest;
        Ok(())
    }

    fn take_u32(cursor: &mut &[u8]) -> core::result::Result<u32, String> {
        let bytes = cursor
            .get(..4)
            .ok_or_else(|| "truncated ssh-agent response".to_string())?;
        *cursor = cursor
            .get(4..)
            .ok_or_else(|| "truncated ssh-agent response".to_string())?;
        Ok(u32::from_be_bytes(
            bytes.try_into().map_err(|_| "invalid ssh-agent integer")?,
        ))
    }

    fn take_string<'a>(cursor: &mut &'a [u8]) -> core::result::Result<&'a [u8], String> {
        let length = take_u32(cursor)? as usize;
        let value = cursor
            .get(..length)
            .ok_or_else(|| "truncated ssh-agent string".to_string())?;
        *cursor = cursor
            .get(length..)
            .ok_or_else(|| "truncated ssh-agent string".to_string())?;
        Ok(value)
    }

    fn put_string(output: &mut Vec<u8>, value: &[u8]) -> core::result::Result<(), String> {
        let length =
            u32::try_from(value.len()).map_err(|_| "ssh-agent string is too large".to_string())?;
        output.extend_from_slice(&length.to_be_bytes());
        output.extend_from_slice(value);
        Ok(())
    }

    fn openssh_fingerprint(public_key: &[u8]) -> String {
        let digest = Sha256::digest(public_key);
        format!("SHA256:{}", base64_no_pad(&digest))
    }

    fn base64_no_pad(input: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let first = chunk[0];
            let second = chunk.get(1).copied().unwrap_or(0);
            let third = chunk.get(2).copied().unwrap_or(0);
            output.push(ALPHABET[(first >> 2) as usize] as char);
            output.push(ALPHABET[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
            if chunk.len() > 1 {
                output.push(ALPHABET[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char);
            }
            if chunk.len() > 2 {
                output.push(ALPHABET[(third & 0x3f) as usize] as char);
            }
        }
        output
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::net::UnixListener;
        use std::thread;

        fn frame(payload: &[u8]) -> Vec<u8> {
            let mut output = Vec::new();
            output.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            output.extend_from_slice(payload);
            output
        }

        fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
            let mut length = [0; 4];
            stream.read_exact(&mut length).unwrap();
            let mut payload = vec![0; u32::from_be_bytes(length) as usize];
            stream.read_exact(&mut payload).unwrap();
            payload
        }

        #[test]
        fn native_transport_lists_and_signs_without_exporting_a_secret() {
            let temporary = std::env::temp_dir().join(format!(
                "pass-ssh-agent-{}-{}.sock",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = std::fs::remove_file(&temporary);
            let listener = UnixListener::bind(&temporary).unwrap();
            let mut public_key = Vec::new();
            put_string(&mut public_key, b"ssh-ed25519").unwrap();
            put_string(&mut public_key, &[7; 32]).unwrap();
            let expected_key = public_key.clone();

            let server = thread::spawn(move || {
                let (mut identities, _) = listener.accept().unwrap();
                assert_eq!(read_frame(&mut identities), vec![REQUEST_IDENTITIES]);
                let mut answer = vec![IDENTITIES_ANSWER];
                answer.extend_from_slice(&1u32.to_be_bytes());
                put_string(&mut answer, &expected_key).unwrap();
                put_string(&mut answer, b"test").unwrap();
                identities.write_all(&frame(&answer)).unwrap();

                let (mut signer, _) = listener.accept().unwrap();
                let request = read_frame(&mut signer);
                assert_eq!(request.first(), Some(&SIGN_REQUEST));
                let mut cursor = &request[1..];
                assert_eq!(take_string(&mut cursor).unwrap(), expected_key);
                assert_eq!(take_string(&mut cursor).unwrap(), b"payload");
                assert_eq!(take_u32(&mut cursor).unwrap(), 0);

                let mut signature_blob = Vec::new();
                put_string(&mut signature_blob, b"ssh-ed25519").unwrap();
                put_string(&mut signature_blob, &[9; 64]).unwrap();
                let mut response = vec![SIGN_RESPONSE];
                put_string(&mut response, &signature_blob).unwrap();
                signer.write_all(&frame(&response)).unwrap();
            });

            let agent = UnixSshAgent::new(&temporary);
            let keys = futures_lite::future::block_on(agent.keys()).unwrap();
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].algorithm, "ssh-ed25519");
            assert!(keys[0].fingerprint.starts_with("SHA256:"));
            let signature =
                futures_lite::future::block_on(agent.sign(&keys[0].public_key, b"payload"))
                    .unwrap();
            assert_eq!(signature, vec![9; 64]);

            server.join().unwrap();
            std::fs::remove_file(temporary).unwrap();
        }
    }
}

#[cfg(all(feature = "unix-ssh-agent", unix))]
pub use unix::UnixSshAgent;

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
