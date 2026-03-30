//! Ledger hardware wallet transport.
//!
//! Defines an APDU-based transport trait and a [`LedgerSigner`] that
//! implements [`Signer`] by forwarding signing requests to a Ledger device.
//! The consumer provides the transport implementation (USB HID, Bluetooth, etc).
//!
//! ```ignore
//! // Implement Transport for your USB backend
//! struct UsbTransport { /* hidapi handle */ }
//! impl Transport for UsbTransport {
//!     type Error = std::io::Error;
//!     async fn exchange(&self, command: &APDUCommand, buf: &mut [u8]) -> Result<usize, Self::Error> {
//!         // send command, read response into buf, return length
//!     }
//! }
//!
//! let signer = LedgerSigner::new(UsbTransport::new()?, "m/44'/60'/0'/0/0")?;
//! wallet.add(signer);
//! wallet.sign(tx).await?;
//! ```

use arrayvec::ArrayString;
use crate::{Signer, SigningError, Signature};

const MAX_ID_LEN: usize = 24;
const MAX_SIG_LEN: usize = 65;
const MAX_APDU_LEN: usize = 260;

/// A raw APDU command to send to a Ledger device.
pub struct APDUCommand<'a> {
    pub cla: u8,
    pub ins: u8,
    pub p1: u8,
    pub p2: u8,
    pub data: &'a [u8],
}

impl APDUCommand<'_> {
    /// Serialize the command into wire format.
    /// Returns `None` if `data` exceeds 255 bytes or `buf` is too small.
    pub fn serialize(&self, buf: &mut [u8]) -> Option<usize> {
        let len = self.data.len();
        if len > 255 || buf.len() < 5 + len {
            return None;
        }
        buf[0] = self.cla;
        buf[1] = self.ins;
        buf[2] = self.p1;
        buf[3] = self.p2;
        buf[4] = len as u8;
        buf[5..5 + len].copy_from_slice(self.data);
        Some(5 + len)
    }
}

/// Transport trait for communicating with a Ledger device.
///
/// Implement this for your USB HID / Bluetooth / TCP backend.
/// The exchange method sends an APDU command and reads the response
/// into the provided buffer, returning the number of bytes read.
pub trait Transport {
    type Error: core::fmt::Debug;

    /// Send an APDU command and read the response.
    /// Returns the number of response bytes written to `buf`.
    /// The last 2 bytes of the response are the status word (SW).
    fn exchange(
        &self,
        command: &APDUCommand,
        buf: &mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, Self::Error>>;
}

/// A signer backed by a Ledger hardware wallet.
pub struct LedgerSigner<T: Transport> {
    transport: T,
    id: ArrayString<MAX_ID_LEN>,
    path_bytes: [u8; 20],
    path_len: u8,
}

/// Parsed APDU response status.
const SW_OK: u16 = 0x9000;

fn parse_status(buf: &[u8], len: usize) -> (u16, usize) {
    if len < 2 {
        return (0, 0);
    }
    let sw = u16::from_be_bytes([buf[len - 2], buf[len - 1]]);
    (sw, len - 2)
}

#[derive(Debug, PartialEq)]
pub struct LedgerSignature {
    bytes: [u8; MAX_SIG_LEN],
    len: u8,
}

impl AsRef<[u8]> for LedgerSignature {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

impl Signature for LedgerSignature {}

#[derive(Debug)]
pub enum LedgerError {
    Transport,
    InvalidPath,
    DeviceError(u16),
}

impl core::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            LedgerError::Transport => write!(f, "Ledger transport error"),
            LedgerError::InvalidPath => write!(f, "Invalid derivation path"),
            LedgerError::DeviceError(code) => write!(f, "Ledger device error: 0x{:04X}", code),
        }
    }
}

impl<T: Transport> LedgerSigner<T> {
    /// Create a new Ledger signer for the given derivation path.
    pub fn new(transport: T, path: &str) -> Result<Self, LedgerError> {
        let mut path_bytes = [0u8; 20];
        let mut path_len = 0u8;

        for segment in path.split('/') {
            match segment {
                "m" | "" => continue,
                s => {
                    if path_len >= 20 {
                        return Err(LedgerError::InvalidPath);
                    }
                    let (num, hardened) = if let Some(n) = s.strip_suffix('\'') {
                        (n, true)
                    } else {
                        (s, false)
                    };
                    let index: u32 = num.parse().map_err(|_| LedgerError::InvalidPath)?;
                    let index = if hardened { index | 0x80000000 } else { index };
                    let offset = path_len as usize;
                    path_bytes[offset..offset + 4].copy_from_slice(&index.to_be_bytes());
                    path_len += 4;
                }
            }
        }

        let mut id = ArrayString::new();
        let len = path.len().min(MAX_ID_LEN);
        let _ = id.try_push_str(&path[..len]);

        Ok(LedgerSigner { transport, id, path_bytes, path_len })
    }

    async fn apdu(&self, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Result<([u8; MAX_APDU_LEN], usize), LedgerError> {
        let command = APDUCommand { cla, ins, p1, p2, data };
        let mut buf = [0u8; MAX_APDU_LEN];
        let len = self.transport.exchange(&command, &mut buf).await
            .map_err(|_| LedgerError::Transport)?;

        let (sw, data_len) = parse_status(&buf, len);
        if sw != SW_OK {
            return Err(LedgerError::DeviceError(sw));
        }
        Ok((buf, data_len))
    }
}

impl<T: Transport> Signer for LedgerSigner<T> {
    type Signature = LedgerSignature;

    fn account_id(&self) -> &str {
        &self.id
    }

    async fn sign_msg(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature, SigningError> {
        let msg = data.as_ref();

        // Build payload: [num_components, path_bytes..., message...]
        let path_end = 1 + self.path_len as usize;
        let msg_end = path_end + msg.len();
        // APDU data field is limited to 255 bytes
        if msg_end > 255 {
            return Err(SigningError::Locked);
        }
        let mut payload = [0u8; MAX_APDU_LEN];
        payload[0] = self.path_len / 4;
        payload[1..path_end].copy_from_slice(&self.path_bytes[..self.path_len as usize]);
        payload[path_end..msg_end].copy_from_slice(msg);

        let (response, data_len) = self.apdu(0xE0, 0x04, 0x00, 0x00, &payload[..msg_end]).await
            .map_err(|_| SigningError::Locked)?;

        let mut bytes = [0u8; MAX_SIG_LEN];
        let len = data_len.min(MAX_SIG_LEN);
        bytes[..len].copy_from_slice(&response[..len]);

        Ok(LedgerSignature { bytes, len: len as u8 })
    }

    async fn verify(&self, _msg: impl AsRef<[u8]>, _sig: impl AsRef<[u8]>) -> bool {
        false
    }
}

impl<T: Transport> core::fmt::Debug for LedgerSigner<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LedgerSigner")
            .field("id", &self.id.as_str())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Wallet;
    use core::cell::RefCell;

    /// Mock transport that returns canned responses.
    struct MockTransport {
        responses: RefCell<arrayvec::ArrayVec<([u8; MAX_APDU_LEN], usize), 4>>,
        last_command: RefCell<Option<([u8; MAX_APDU_LEN], usize)>>,
    }

    impl MockTransport {
        fn with_response(data: &[u8], sw: u16) -> Self {
            let mut buf = [0u8; MAX_APDU_LEN];
            let len = data.len();
            buf[..len].copy_from_slice(data);
            buf[len] = (sw >> 8) as u8;
            buf[len + 1] = sw as u8;
            let mut responses = arrayvec::ArrayVec::new();
            responses.push((buf, len + 2));
            MockTransport {
                responses: RefCell::new(responses),
                last_command: RefCell::new(None),
            }
        }

        fn empty_ok() -> Self {
            Self::with_response(&[], SW_OK)
        }

        fn last_payload(&self) -> Option<([u8; MAX_APDU_LEN], usize)> {
            *self.last_command.borrow()
        }
    }

    impl Transport for MockTransport {
        type Error = &'static str;

        async fn exchange(
            &self,
            command: &APDUCommand<'_>,
            buf: &mut [u8],
        ) -> Result<usize, Self::Error> {
            // Capture the serialized command
            let mut cmd_buf = [0u8; MAX_APDU_LEN];
            let cmd_len = command.serialize(&mut cmd_buf).unwrap_or(0);
            *self.last_command.borrow_mut() = Some((cmd_buf, cmd_len));

            let mut responses = self.responses.borrow_mut();
            let (resp_buf, resp_len) = if responses.is_empty() {
                let mut b = [0u8; MAX_APDU_LEN];
                b[0] = 0x90;
                b[1] = 0x00;
                (b, 2)
            } else {
                responses.remove(0)
            };

            buf[..resp_len].copy_from_slice(&resp_buf[..resp_len]);
            Ok(resp_len)
        }
    }

    #[test]
    fn path_parsing_ethereum() {
        let transport = MockTransport::empty_ok();
        let signer = LedgerSigner::new(transport, "m/44'/60'/0'/0/0").unwrap();

        assert_eq!(signer.account_id(), "m/44'/60'/0'/0/0");
        assert_eq!(signer.path_len, 20); // 5 × 4

        let expected: [u32; 5] = [0x8000002C, 0x8000003C, 0x80000000, 0, 0];
        for (i, &exp) in expected.iter().enumerate() {
            let offset = i * 4;
            let val = u32::from_be_bytes(
                signer.path_bytes[offset..offset + 4].try_into().unwrap()
            );
            assert_eq!(val, exp, "path component {}", i);
        }
    }

    #[test]
    fn path_parsing_bitcoin() {
        let transport = MockTransport::empty_ok();
        let signer = LedgerSigner::new(transport, "m/84'/0'/0'/0/0").unwrap();
        assert_eq!(signer.path_len, 20);
    }

    #[test]
    fn invalid_path_rejected() {
        let transport = MockTransport::empty_ok();
        assert!(LedgerSigner::new(transport, "m/not/a/number").is_err());
    }

    #[test]
    fn path_too_deep_rejected() {
        let transport = MockTransport::empty_ok();
        assert!(LedgerSigner::new(transport, "m/44'/60'/0'/0/0/0").is_err());
    }

    #[async_std::test]
    async fn sign_returns_device_signature() {
        let fake_sig = [0xAB; 65];
        let transport = MockTransport::with_response(&fake_sig, SW_OK);
        let signer = LedgerSigner::new(transport, "m/44'/60'/0'/0/0").unwrap();

        let sig = signer.sign_msg(b"hello ledger").await.unwrap();
        assert_eq!(sig.as_ref().len(), 65);
        assert!(sig.as_ref().iter().all(|&b| b == 0xAB));
    }

    #[async_std::test]
    async fn sign_sends_path_and_message() {
        let transport = MockTransport::with_response(&[0; 64], SW_OK);
        let signer = LedgerSigner::new(transport, "m/44'/60'/0'/0/0").unwrap();

        let _ = signer.sign_msg(b"test").await;

        // Verify the APDU that was sent
        let (cmd, cmd_len) = signer.transport.last_payload().unwrap();
        assert_eq!(cmd[0], 0xE0); // CLA
        assert_eq!(cmd[1], 0x04); // INS

        // Payload starts at byte 5 (after CLA/INS/P1/P2/Lc)
        let payload = &cmd[5..cmd_len];
        assert_eq!(payload[0], 5); // 5 path components
        // Message starts after path (1 + 20 = 21 bytes in)
        assert_eq!(&payload[21..25], b"test");
    }

    #[async_std::test]
    async fn device_rejection_returns_error() {
        // 0x6985 = user rejected on device
        let transport = MockTransport::with_response(&[], 0x6985);
        let signer = LedgerSigner::new(transport, "m/44'/60'/0'/0/0").unwrap();

        let result = signer.sign_msg(b"rejected").await;
        assert!(result.is_err());
    }

    #[async_std::test]
    async fn wallet_integration() {
        let fake_sig = [0x42; 65];
        let transport = MockTransport::with_response(&fake_sig, SW_OK);
        let signer = LedgerSigner::new(transport, "m/44'/60'/0'/0/0").unwrap();

        let mut wallet: Wallet<_, 5> = Wallet::new();
        wallet.add(signer);

        assert_eq!(wallet.accounts_len(), 1);
        assert_eq!(wallet.default_account().unwrap().name(), "m/44'/60'/0'/0/0");

        let sig = wallet.sign(b"tx payload").await.unwrap();
        assert_eq!(sig.as_ref().len(), 65);
        assert!(sig.as_ref().iter().all(|&b| b == 0x42));
    }
}
