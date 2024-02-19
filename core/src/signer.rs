use async_trait::async_trait;
use serde::Serialize;

pub enum SignerError {
    Unknown,
    WrongCredentials,
    Platform(String),
}

pub type SignerResult<T> = Result<T, SignerError>;

#[async_trait(?Send)]
pub trait Signer {
    type SignedPayload: Serialize;
    async fn sign<'p>(&self, payload: &'p [u8]) -> SignerResult<Self::SignedPayload>;
}
