//! Sign-only backends for hardware wallets and remote signers.
//!
//! These never hold private keys — they proxy signing requests to an
//! external device or service. The consumer provides the transport.
//!
//! ```ignore
//! // Ledger via USB
//! let signer = ProxySigner::new("ledger-0", |msg| {
//!     ledger_send_apdu(msg) // your transport
//! });
//!
//! // QR-code air-gapped signer
//! let signer = ProxySigner::new("vault-0", |msg| {
//!     display_qr(msg);
//!     scan_qr_response()
//! });
//!
//! wallet.add(signer);
//! wallet.sign(payload).await?;
//! ```

use arrayvec::ArrayString;
use crate::{Signer, SigningError, Signature};

const MAX_SIG_LEN: usize = 65; // covers secp256k1 (65), ed25519 (64), sr25519 (64)
type SigBytes = [u8; MAX_SIG_LEN];

const MAX_ID_LEN: usize = 24;

/// A sign-only signer that delegates to an external function.
/// No private keys are held — the function handles the actual signing
/// (e.g. sending APDU to a Ledger, displaying a QR code, calling a remote API).
pub struct ProxySigner<F>
where
    F: Fn(&[u8]) -> Result<SigBytes, SigningError>,
{
    id: ArrayString<MAX_ID_LEN>,
    sig_len: u8,
    sign_fn: F,
}

impl<F> ProxySigner<F>
where
    F: Fn(&[u8]) -> Result<SigBytes, SigningError>,
{
    /// Create a proxy signer with the given name and signing function.
    /// `sig_len` is the expected signature length (64 for ed25519/sr25519, 65 for secp256k1).
    pub fn new(id: &str, sig_len: u8, sign_fn: F) -> Self {
        let mut name = ArrayString::new();
        let len = id.len().min(MAX_ID_LEN);
        let _ = name.try_push_str(&id[..len]);
        ProxySigner { id: name, sig_len, sign_fn }
    }

    /// Convenience: create a Ledger-style proxy (secp256k1, 65-byte signatures).
    pub fn ledger(index: u8, sign_fn: F) -> Self {
        let mut id = ArrayString::new();
        let _ = id.try_push_str("ledger-");
        // simple u8 to char
        if index >= 10 {
            id.push((b'0' + index / 10) as char);
        }
        id.push((b'0' + index % 10) as char);
        ProxySigner { id, sig_len: 65, sign_fn }
    }

    /// Convenience: create a Polkadot Vault (air-gapped) proxy (sr25519, 64-byte signatures).
    pub fn polkadot_vault(index: u8, sign_fn: F) -> Self {
        let mut id = ArrayString::new();
        let _ = id.try_push_str("vault-");
        if index >= 10 {
            id.push((b'0' + index / 10) as char);
        }
        id.push((b'0' + index % 10) as char);
        ProxySigner { id, sig_len: 64, sign_fn }
    }
}

/// Fixed-size signature wrapper for proxy signers.
#[derive(Debug, PartialEq)]
pub struct ProxySignature {
    bytes: SigBytes,
    len: u8,
}

impl AsRef<[u8]> for ProxySignature {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

impl Signature for ProxySignature {}

impl<F> Signer for ProxySigner<F>
where
    F: Fn(&[u8]) -> Result<SigBytes, SigningError>,
{
    type Signature = ProxySignature;

    fn account_id(&self) -> &str {
        &self.id
    }

    async fn sign_msg(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature, SigningError> {
        let bytes = (self.sign_fn)(data.as_ref())?;
        Ok(ProxySignature { bytes, len: self.sig_len })
    }

    async fn verify(&self, _msg: impl AsRef<[u8]>, _sig: impl AsRef<[u8]>) -> bool {
        // Proxy signers typically can't verify — the device doesn't expose that.
        // Verification should be done using the public key directly.
        false
    }
}

impl<F> core::fmt::Debug for ProxySigner<F>
where
    F: Fn(&[u8]) -> Result<SigBytes, SigningError>,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProxySigner")
            .field("id", &self.id.as_str())
            .finish()
    }
}
