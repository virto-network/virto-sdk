use crate::{Network, Signer, SigningError};

/// An account wraps any signer with wallet metadata.
/// The account name is derived from the signer's own identity.
pub struct Account<S: Signer> {
    signer: S,
    network: Network,
}

impl<S: Signer> Account<S> {
    pub fn new(signer: S) -> Self {
        Account {
            signer,
            network: Network::default(),
        }
    }

    pub fn name(&self) -> &str {
        self.signer.account_id()
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

    fn account_id(&self) -> &str {
        self.signer.account_id()
    }

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
            .field("name", &self.name())
            .field("network", &self.network)
            .field("signer", &self.signer)
            .finish()
    }
}
