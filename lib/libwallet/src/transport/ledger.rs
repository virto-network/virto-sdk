//! Ledger hardware wallet transport.
//!
//! Provides a [`LedgerSigner`] that communicates with Ledger devices
//! via the APDU protocol using the `ledger-transport` crate.
//!
//! ```ignore
//! use libwallet::transport::ledger::LedgerSigner;
//!
//! let signer = LedgerSigner::new(transport, "m/44'/60'/0'/0/0").await?;
//! wallet.add(signer);
//! wallet.sign(tx).await?;
//! ```

use arrayvec::ArrayString;
use crate::{Signer, SigningError, Signature};

const MAX_ID_LEN: usize = 24;
const MAX_SIG_LEN: usize = 65;

/// A signer backed by a Ledger hardware wallet.
///
/// Holds a reference to the transport and the BIP32 derivation path.
/// Signing requests are forwarded to the device via APDU commands.
pub struct LedgerSigner<T: ledger_transport::Exchange> {
    transport: T,
    id: ArrayString<MAX_ID_LEN>,
    path_bytes: [u8; 20], // up to 5 BIP32 levels × 4 bytes
    path_len: u8,
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

impl<T: ledger_transport::Exchange> LedgerSigner<T> {
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

    /// Send a raw APDU command to the device.
    async fn apdu(&self, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Result<std::vec::Vec<u8>, LedgerError> {
        let command: ledger_apdu::APDUCommand<std::vec::Vec<u8>> = ledger_apdu::APDUCommand {
            cla,
            ins,
            p1,
            p2,
            data: data.to_vec(),
        };
        let response = self.transport.exchange(&command).await
            .map_err(|_| LedgerError::Transport)?;

        if response.retcode() != 0x9000 {
            return Err(LedgerError::DeviceError(response.retcode()));
        }
        Ok(response.data().to_vec())
    }
}

#[derive(Debug)]
pub enum LedgerError {
    Transport,
    InvalidPath,
    DeviceError(u16),
    SigningFailed,
}

impl core::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            LedgerError::Transport => write!(f, "Ledger transport error"),
            LedgerError::InvalidPath => write!(f, "Invalid derivation path"),
            LedgerError::DeviceError(code) => write!(f, "Ledger device error: 0x{:04X}", code),
            LedgerError::SigningFailed => write!(f, "Signing failed"),
        }
    }
}

impl<T: ledger_transport::Exchange> Signer for LedgerSigner<T> {
    type Signature = LedgerSignature;

    fn account_id(&self) -> &str {
        &self.id
    }

    async fn sign_msg(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature, SigningError> {
        // Generic signing: send path + message to device
        // CLA/INS vary by app (Ethereum: 0xE0/0x04, Bitcoin: 0xE0/0x48)
        // This sends a generic sign request — app-specific wrappers can override
        let msg = data.as_ref();
        let mut payload = std::vec::Vec::with_capacity(1 + self.path_len as usize + msg.len());
        payload.push(self.path_len / 4); // number of path components
        payload.extend_from_slice(&self.path_bytes[..self.path_len as usize]);
        payload.extend_from_slice(msg);

        let response = self.apdu(0xE0, 0x04, 0x00, 0x00, &payload).await
            .map_err(|_| SigningError::Locked)?;

        let mut bytes = [0u8; MAX_SIG_LEN];
        let len = response.len().min(MAX_SIG_LEN);
        bytes[..len].copy_from_slice(&response[..len]);

        Ok(LedgerSignature { bytes, len: len as u8 })
    }

    async fn verify(&self, _msg: impl AsRef<[u8]>, _sig: impl AsRef<[u8]>) -> bool {
        false // hardware wallets don't expose verification
    }
}

impl<T: ledger_transport::Exchange> core::fmt::Debug for LedgerSigner<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LedgerSigner")
            .field("id", &self.id.as_str())
            .finish()
    }
}
