use async_trait::async_trait;
use core::fmt::Debug;
use serde::{de::DeserializeOwned, Serialize};

pub enum SignerError {
    Unknown,
    WrongCredentials,
    Platform(String),
}

pub type WalletResult<T> = Result<T, SignerError>;
pub trait WalletApiSignedPayloadBounds =
    Sync + Send + DeserializeOwned + Serialize + PartialEq + Clone + Debug;
trait WA = WalletApi;

pub struct WalletServices<A: WA> {
    pub services: A,
}

impl<A: WA> WalletServices<A> {
    pub fn new(services: A) -> Self {
        Self { services }
    }
}

#[async_trait]
pub trait WalletApi {
    type SignedPayload: WalletApiSignedPayloadBounds;
    async fn sign<'p>(&self, payload: &'p [u8]) -> WalletResult<Self::SignedPayload>;
}
