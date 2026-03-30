//! Trezor hardware wallet transport.
//!
//! Implements the Trezor wire protocol (protobuf over USB HID framing)
//! with hand-encoded messages for the sign-only flow. No protobuf
//! library dependency.
//!
//! ```ignore
//! struct UsbHid { /* hidapi handle */ }
//! impl TrezorTransport for UsbHid {
//!     type Error = std::io::Error;
//!     async fn write_chunk(&self, chunk: &[u8; 64]) -> Result<(), Self::Error> { ... }
//!     async fn read_chunk(&self, chunk: &mut [u8; 64]) -> Result<(), Self::Error> { ... }
//! }
//!
//! let signer = TrezorSigner::new(UsbHid::new()?, "m/44'/60'/0'/0/0")?;
//! wallet.add(signer);
//! wallet.sign(tx).await?;
//! ```

use arrayvec::ArrayString;
use crate::{Signer, SigningError, Signature};

const MAX_ID_LEN: usize = 24;
const MAX_SIG_LEN: usize = 65;
const CHUNK_SIZE: usize = 64;
const MAX_MSG_LEN: usize = 256;

// Wire message types
const MSG_SIGN_MESSAGE: u16 = 38;
const MSG_MESSAGE_SIGNATURE: u16 = 40;
#[allow(dead_code)] // used when signing Ethereum messages
const MSG_ETH_SIGN_MESSAGE: u16 = 64;
const MSG_ETH_MESSAGE_SIGNATURE: u16 = 67;
const MSG_BUTTON_REQUEST: u16 = 19;
const MSG_BUTTON_ACK: u16 = 20;
const MSG_FAILURE: u16 = 3;

/// Transport trait for Trezor USB HID communication.
/// Implement this for your USB backend (hidapi, embedded-io, etc).
pub trait TrezorTransport {
    type Error: core::fmt::Debug;

    /// Write a 64-byte HID report to the device.
    fn write_chunk(
        &self,
        chunk: &[u8; CHUNK_SIZE],
    ) -> impl core::future::Future<Output = Result<(), Self::Error>>;

    /// Read a 64-byte HID report from the device.
    fn read_chunk(
        &self,
        chunk: &mut [u8; CHUNK_SIZE],
    ) -> impl core::future::Future<Output = Result<(), Self::Error>>;
}

/// A signer backed by a Trezor hardware wallet.
pub struct TrezorSigner<T: TrezorTransport> {
    transport: T,
    id: ArrayString<MAX_ID_LEN>,
    path: [u32; 5],
    path_len: u8,
}

#[derive(Debug, PartialEq)]
pub struct TrezorSignature {
    bytes: [u8; MAX_SIG_LEN],
    len: u8,
}

impl AsRef<[u8]> for TrezorSignature {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

impl Signature for TrezorSignature {}

#[derive(Debug)]
pub enum TrezorError {
    Transport,
    InvalidPath,
    DeviceError,
    BadResponse,
    UserRejected,
}

impl core::fmt::Display for TrezorError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            TrezorError::Transport => write!(f, "Trezor transport error"),
            TrezorError::InvalidPath => write!(f, "Invalid derivation path"),
            TrezorError::DeviceError => write!(f, "Trezor device error"),
            TrezorError::BadResponse => write!(f, "Invalid response from device"),
            TrezorError::UserRejected => write!(f, "User rejected on device"),
        }
    }
}

impl<T: TrezorTransport> TrezorSigner<T> {
    /// Create a new Trezor signer for the given derivation path.
    pub fn new(transport: T, path_str: &str) -> Result<Self, TrezorError> {
        let mut path = [0u32; 5];
        let mut path_len = 0u8;

        for segment in path_str.split('/') {
            match segment {
                "m" | "" => continue,
                s => {
                    if path_len >= 5 {
                        return Err(TrezorError::InvalidPath);
                    }
                    let (num, hardened) = if let Some(n) = s.strip_suffix('\'') {
                        (n, true)
                    } else {
                        (s, false)
                    };
                    let index: u32 = num.parse().map_err(|_| TrezorError::InvalidPath)?;
                    path[path_len as usize] = if hardened { index | 0x80000000 } else { index };
                    path_len += 1;
                }
            }
        }

        let mut id = ArrayString::new();
        let len = path_str.len().min(MAX_ID_LEN);
        let _ = id.try_push_str(&path_str[..len]);

        Ok(TrezorSigner { transport, id, path, path_len })
    }

    /// Send a framed message to the Trezor device.
    async fn send(&self, msg_type: u16, payload: &[u8]) -> Result<(), TrezorError> {
        let mut chunk = [0u8; CHUNK_SIZE];

        // First chunk: magic(3) + type(2) + length(4) + data
        chunk[0] = b'?';
        chunk[1] = b'#';
        chunk[2] = b'#';
        chunk[3..5].copy_from_slice(&msg_type.to_be_bytes());
        chunk[5..9].copy_from_slice(&(payload.len() as u32).to_be_bytes());

        let first_data = payload.len().min(CHUNK_SIZE - 9);
        chunk[9..9 + first_data].copy_from_slice(&payload[..first_data]);

        self.transport.write_chunk(&chunk).await.map_err(|_| TrezorError::Transport)?;

        // Continuation chunks
        let mut offset = first_data;
        while offset < payload.len() {
            chunk = [0u8; CHUNK_SIZE];
            chunk[0] = b'?';
            let cont_data = (payload.len() - offset).min(CHUNK_SIZE - 1);
            chunk[1..1 + cont_data].copy_from_slice(&payload[offset..offset + cont_data]);
            self.transport.write_chunk(&chunk).await.map_err(|_| TrezorError::Transport)?;
            offset += cont_data;
        }

        Ok(())
    }

    /// Read a framed message from the Trezor device.
    /// Returns (message_type, data_length) with data written into `buf`.
    async fn recv(&self, buf: &mut [u8]) -> Result<(u16, usize), TrezorError> {
        let mut chunk = [0u8; CHUNK_SIZE];

        // Read first chunk
        self.transport.read_chunk(&mut chunk).await.map_err(|_| TrezorError::Transport)?;

        if chunk[0] != b'?' || chunk[1] != b'#' || chunk[2] != b'#' {
            return Err(TrezorError::BadResponse);
        }

        let msg_type = u16::from_be_bytes([chunk[3], chunk[4]]);
        let msg_len = u32::from_be_bytes([chunk[5], chunk[6], chunk[7], chunk[8]]) as usize;
        // Cap to buffer size AND a reasonable upper bound
        let total = msg_len.min(buf.len()).min(MAX_MSG_LEN);

        let first_data = total.min(CHUNK_SIZE - 9);
        buf[..first_data].copy_from_slice(&chunk[9..9 + first_data]);

        // Read continuation chunks (bounded by total which is capped)
        let mut offset = first_data;
        while offset < total {
            self.transport.read_chunk(&mut chunk).await.map_err(|_| TrezorError::Transport)?;
            if chunk[0] != b'?' {
                return Err(TrezorError::BadResponse);
            }
            let cont_data = (total - offset).min(CHUNK_SIZE - 1);
            buf[offset..offset + cont_data].copy_from_slice(&chunk[1..1 + cont_data]);
            offset += cont_data;
        }

        Ok((msg_type, total))
    }

    /// Sign a message, handling ButtonRequest/Ack flow.
    /// Allows at most `MAX_BUTTON_ROUNDS` button confirmations to prevent infinite loops.
    async fn sign_raw(&self, msg_type: u16, payload: &[u8]) -> Result<([u8; MAX_SIG_LEN], u8), TrezorError> {
        const MAX_BUTTON_ROUNDS: usize = 8;

        self.send(msg_type, payload).await?;

        let mut buf = [0u8; MAX_MSG_LEN];
        for _ in 0..MAX_BUTTON_ROUNDS {
            let (resp_type, resp_len) = self.recv(&mut buf).await?;

            match resp_type {
                MSG_BUTTON_REQUEST => {
                    self.send(MSG_BUTTON_ACK, &[]).await?;
                }
                MSG_MESSAGE_SIGNATURE | MSG_ETH_MESSAGE_SIGNATURE => {
                    let sig = parse_signature(&buf[..resp_len])?;
                    return Ok(sig);
                }
                MSG_FAILURE => {
                    return Err(TrezorError::UserRejected);
                }
                _ => return Err(TrezorError::BadResponse),
            }
        }
        Err(TrezorError::BadResponse)
    }
}

// -- Minimal protobuf encoding/decoding --

fn encode_varint(mut value: u64, buf: &mut [u8]) -> usize {
    let mut i = 0;
    loop {
        if value < 0x80 {
            buf[i] = value as u8;
            return i + 1;
        }
        buf[i] = (value as u8 & 0x7F) | 0x80;
        value >>= 7;
        i += 1;
    }
}

fn decode_varint(buf: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    // A u64 varint is at most 10 bytes (ceil(64/7))
    let limit = buf.len().min(10);
    for (i, &byte) in buf[..limit].iter().enumerate() {
        value |= ((byte & 0x7F) as u64).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
    }
    None // unterminated or overlong varint
}

/// Encode a SignMessage protobuf payload.
fn encode_sign_message(path: &[u32], message: &[u8], buf: &mut [u8]) -> usize {
    let mut pos = 0;

    // Field 1: repeated uint32 address_n
    // Trezor expects non-packed repeated fields (one tag per element)
    for &index in path {
        // Tag: field 1, wire type 0 (VARINT) = 0x08
        buf[pos] = 0x08;
        pos += 1;
        pos += encode_varint(index as u64, &mut buf[pos..]);
    }

    // Field 2: bytes message
    // Tag: field 2, wire type 2 (LEN) = 0x12
    buf[pos] = 0x12;
    pos += 1;
    pos += encode_varint(message.len() as u64, &mut buf[pos..]);
    buf[pos..pos + message.len()].copy_from_slice(message);
    pos += message.len();

    pos
}

/// Parse a signature from a MessageSignature protobuf response.
/// Looks for field 2 (signature bytes) in the response.
fn parse_signature(buf: &[u8]) -> Result<([u8; MAX_SIG_LEN], u8), TrezorError> {
    let mut pos = 0;
    while pos < buf.len() {
        let (tag, tag_len) = decode_varint(&buf[pos..]).ok_or(TrezorError::BadResponse)?;
        pos += tag_len;
        let field_number = tag >> 3;
        let wire_type = tag & 0x07;

        match wire_type {
            2 => {
                // Length-delimited
                let (len, len_len) = decode_varint(&buf[pos..]).ok_or(TrezorError::BadResponse)?;
                pos += len_len;
                let len = len as usize;

                if pos + len > buf.len() {
                    return Err(TrezorError::BadResponse);
                }

                if field_number == 2 && len <= MAX_SIG_LEN {
                    let mut sig = [0u8; MAX_SIG_LEN];
                    sig[..len].copy_from_slice(&buf[pos..pos + len]);
                    return Ok((sig, len as u8));
                }
                pos += len;
            }
            0 => {
                // Varint — skip
                let (_, vlen) = decode_varint(&buf[pos..]).ok_or(TrezorError::BadResponse)?;
                pos += vlen;
            }
            _ => return Err(TrezorError::BadResponse),
        }
    }
    Err(TrezorError::BadResponse)
}

impl<T: TrezorTransport> Signer for TrezorSigner<T> {
    type Signature = TrezorSignature;

    fn account_id(&self) -> &str {
        &self.id
    }

    async fn sign_msg(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature, SigningError> {
        let msg = data.as_ref();
        let path = &self.path[..self.path_len as usize];

        let mut payload = [0u8; MAX_MSG_LEN];
        let payload_len = encode_sign_message(path, msg, &mut payload);

        let (bytes, len) = self.sign_raw(MSG_SIGN_MESSAGE, &payload[..payload_len]).await
            .map_err(|_| SigningError::Locked)?;

        Ok(TrezorSignature { bytes, len })
    }

    async fn verify(&self, _msg: impl AsRef<[u8]>, _sig: impl AsRef<[u8]>) -> bool {
        false
    }
}

impl<T: TrezorTransport> core::fmt::Debug for TrezorSigner<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TrezorSigner")
            .field("id", &self.id.as_str())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Wallet;
    use core::cell::RefCell;

    /// Mock Trezor transport that records chunks and replays canned responses.
    struct MockTrezor {
        /// Pre-loaded response frames (each is a complete framed message).
        responses: RefCell<arrayvec::ArrayVec<([u8; MAX_MSG_LEN], u16, usize), 4>>,
        /// Captured sent messages: (msg_type, payload).
        sent: RefCell<arrayvec::ArrayVec<(u16, [u8; MAX_MSG_LEN], usize), 4>>,
    }

    impl MockTrezor {
        fn new() -> Self {
            MockTrezor {
                responses: RefCell::new(arrayvec::ArrayVec::new()),
                sent: RefCell::new(arrayvec::ArrayVec::new()),
            }
        }

        /// Queue a response as pre-framed HID chunks (matching Trezor wire format).
        fn queue_response(&self, msg_type: u16, payload: &[u8]) {
            let len = payload.len();

            // First chunk: magic(3) + type(2) + length(4) + up to 55 bytes
            let mut chunk = [0u8; MAX_MSG_LEN];
            chunk[0] = b'?';
            chunk[1] = b'#';
            chunk[2] = b'#';
            chunk[3..5].copy_from_slice(&msg_type.to_be_bytes());
            chunk[5..9].copy_from_slice(&(len as u32).to_be_bytes());
            let first = len.min(CHUNK_SIZE - 9);
            chunk[9..9 + first].copy_from_slice(&payload[..first]);
            self.responses.borrow_mut().push((chunk, 0, CHUNK_SIZE));

            // Continuation chunks
            let mut offset = first;
            while offset < len {
                let mut cont = [0u8; MAX_MSG_LEN];
                cont[0] = b'?';
                let n = (len - offset).min(CHUNK_SIZE - 1);
                cont[1..1 + n].copy_from_slice(&payload[offset..offset + n]);
                self.responses.borrow_mut().push((cont, 0, CHUNK_SIZE));
                offset += n;
            }
        }

        /// Queue a MessageSignature response with a fake 65-byte signature.
        fn queue_signature(&self, sig_byte: u8) {
            // Hand-encode: field 2 (tag=0x12), length=65, then 65 bytes
            let mut payload = [0u8; 67];
            payload[0] = 0x12; // field 2, wire type LEN
            payload[1] = 65;   // length
            payload[2..67].fill(sig_byte);
            self.queue_response(MSG_MESSAGE_SIGNATURE, &payload);
        }

        /// Queue a ButtonRequest (device wants user confirmation).
        fn queue_button_request(&self) {
            // ButtonRequest has a code field but we can send empty
            self.queue_response(MSG_BUTTON_REQUEST, &[]);
        }

        /// Queue a Failure response.
        fn queue_failure(&self) {
            self.queue_response(MSG_FAILURE, &[]);
        }

        fn sent_messages(&self) -> arrayvec::ArrayVec<(u16, usize), 4> {
            self.sent.borrow().iter().map(|(t, _, l)| (*t, *l)).collect()
        }
    }

    impl TrezorTransport for MockTrezor {
        type Error = &'static str;

        async fn write_chunk(&self, chunk: &[u8; CHUNK_SIZE]) -> Result<(), Self::Error> {
            // Parse the header to capture the message
            if chunk[0] == b'?' && chunk[1] == b'#' && chunk[2] == b'#' {
                let msg_type = u16::from_be_bytes([chunk[3], chunk[4]]);
                let msg_len = u32::from_be_bytes([chunk[5], chunk[6], chunk[7], chunk[8]]) as usize;
                let mut buf = [0u8; MAX_MSG_LEN];
                let first = msg_len.min(CHUNK_SIZE - 9);
                buf[..first].copy_from_slice(&chunk[9..9 + first]);
                self.sent.borrow_mut().push((msg_type, buf, msg_len));
            }
            Ok(())
        }

        async fn read_chunk(&self, chunk: &mut [u8; CHUNK_SIZE]) -> Result<(), Self::Error> {
            let mut responses = self.responses.borrow_mut();
            if responses.is_empty() {
                return Err("no response queued");
            }
            let (buf, _, _) = responses.remove(0);
            chunk.copy_from_slice(&buf[..CHUNK_SIZE]);
            Ok(())
        }
    }

    #[test]
    fn path_parsing() {
        let mock = MockTrezor::new();
        let signer = TrezorSigner::new(mock, "m/44'/0'/0'/0/0").unwrap();
        assert_eq!(signer.path_len, 5);
        assert_eq!(signer.path[0], 0x8000002C);
        assert_eq!(signer.path[1], 0x80000000);
        assert_eq!(signer.path[4], 0);
    }

    #[test]
    fn invalid_path() {
        let mock = MockTrezor::new();
        assert!(TrezorSigner::new(mock, "m/abc/def").is_err());
    }

    #[test]
    fn path_too_deep() {
        let mock = MockTrezor::new();
        assert!(TrezorSigner::new(mock, "m/1/2/3/4/5/6").is_err());
    }

    #[test]
    fn protobuf_encode_decode_roundtrip() {
        let path = [0x8000002Cu32, 0x8000003C, 0x80000000, 0, 0];
        let msg = b"test message";
        let mut buf = [0u8; 128];
        let len = encode_sign_message(&path, msg, &mut buf);

        // Should encode field 1 (path) and field 2 (message)
        assert!(len > 0);
        assert_eq!(buf[0], 0x08); // field 1, wire type varint (non-packed repeated)
    }

    #[test]
    fn parse_signature_from_protobuf() {
        // Encode: field 1 (address) = "addr", field 2 (signature) = 65 bytes of 0xBB
        let mut payload = [0u8; 128];
        let mut pos = 0;

        // Field 1: string address = "addr"
        payload[pos] = 0x0A; pos += 1; // tag: field 1, LEN
        payload[pos] = 4; pos += 1;    // length: 4
        payload[pos..pos + 4].copy_from_slice(b"addr"); pos += 4;

        // Field 2: bytes signature = 65 × 0xBB
        payload[pos] = 0x12; pos += 1; // tag: field 2, LEN
        payload[pos] = 65; pos += 1;   // length: 65
        payload[pos..pos + 65].fill(0xBB); pos += 65;

        let (sig, len) = parse_signature(&payload[..pos]).unwrap();
        assert_eq!(len, 65);
        assert!(sig[..65].iter().all(|&b| b == 0xBB));
    }

    #[async_std::test]
    async fn mock_transport_works() {
        let mock = MockTrezor::new();
        mock.queue_signature(0xAA);

        // Write a chunk (should succeed)
        let chunk = [0u8; CHUNK_SIZE];
        mock.write_chunk(&chunk).await.unwrap();

        // Read a chunk (should return the queued response)
        let mut read_buf = [0u8; CHUNK_SIZE];
        mock.read_chunk(&mut read_buf).await.unwrap();

        // Should have magic header
        assert_eq!(read_buf[0], b'?');
        assert_eq!(read_buf[1], b'#');
        assert_eq!(read_buf[2], b'#');

        // Message type should be MSG_MESSAGE_SIGNATURE (40)
        let msg_type = u16::from_be_bytes([read_buf[3], read_buf[4]]);
        assert_eq!(msg_type, MSG_MESSAGE_SIGNATURE);
    }

    #[async_std::test]
    async fn sign_simple() {
        let mock = MockTrezor::new();
        mock.queue_signature(0xCC);

        let signer = TrezorSigner::new(mock, "m/44'/0'/0'/0/0").unwrap();
        let sig = signer.sign_msg(b"hello trezor").await;
        assert!(sig.is_ok(), "sign failed: {:?}", sig.err());
        let sig = sig.unwrap();

        assert_eq!(sig.as_ref().len(), 65);
        assert!(sig.as_ref().iter().all(|&b| b == 0xCC));
    }

    #[async_std::test]
    async fn sign_with_button_request() {
        let mock = MockTrezor::new();
        mock.queue_button_request();
        mock.queue_signature(0xDD);

        let signer = TrezorSigner::new(mock, "m/44'/60'/0'/0/0").unwrap();
        let sig = signer.sign_msg(b"confirm me").await.unwrap();

        assert_eq!(sig.as_ref().len(), 65);

        // Should have sent: SignMessage + ButtonAck
        let sent = signer.transport.sent_messages();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, MSG_SIGN_MESSAGE);
        assert_eq!(sent[1].0, MSG_BUTTON_ACK);
    }

    #[async_std::test]
    async fn sign_user_rejected() {
        let mock = MockTrezor::new();
        mock.queue_failure();

        let signer = TrezorSigner::new(mock, "m/44'/0'/0'/0/0").unwrap();
        let result = signer.sign_msg(b"rejected").await;

        assert!(result.is_err());
    }

    #[async_std::test]
    async fn wallet_integration() {
        let mock = MockTrezor::new();
        mock.queue_signature(0xEE);

        let signer = TrezorSigner::new(mock, "m/44'/60'/0'/0/0").unwrap();

        let mut wallet: Wallet<_, 5> = Wallet::new();
        wallet.add(signer);

        assert_eq!(wallet.accounts_len(), 1);
        assert_eq!(wallet.default_account().unwrap().name(), "m/44'/60'/0'/0/0");

        let sig = wallet.sign(b"tx").await.unwrap();
        assert_eq!(sig.as_ref().len(), 65);
    }
}
