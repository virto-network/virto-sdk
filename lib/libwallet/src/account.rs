use arrayvec::ArrayString;
use crate::{Network, Signer, SigningError};

const MAX_NAME_LEN: usize = 14;

/// An account wraps any signer with wallet metadata (name, network).
pub struct Account<S: Signer> {
    signer: S,
    name: ArrayString<MAX_NAME_LEN>,
    network: Network,
}

impl<S: Signer> Account<S> {
    pub fn new(name: &str, signer: S) -> Self {
        let mut name_buf = ArrayString::new();
        for ch in name.chars() {
            if name_buf.is_full() {
                break;
            }
            name_buf.push(ch);
        }
        if name_buf.is_empty() {
            let _ = name_buf.try_push_str("default");
        }
        Account {
            signer,
            name: name_buf,
            network: Network::default(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn switch_network(mut self, net: impl Into<Network>) -> Self {
        self.network = net.into();
        self
    }

    pub fn signer(&self) -> &S {
        &self.signer
    }
}

impl<S: Signer> Signer for Account<S> {
    type Signature = S::Signature;

    async fn sign_msg(&self, data: impl AsRef<[u8]>) -> Result<Self::Signature, SigningError> {
        self.signer.sign_msg(data).await
    }

    async fn verify(&self, msg: impl AsRef<[u8]>, sig: impl AsRef<[u8]>) -> bool {
        self.signer.verify(msg, sig).await
    }
}

impl<S: Signer + core::fmt::Debug> core::fmt::Debug for Account<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Account")
            .field("name", &self.name.as_str())
            .field("network", &self.network)
            .field("signer", &self.signer)
            .finish()
    }
}
