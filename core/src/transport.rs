use async_trait::async_trait;
use core::fmt::Debug;
use serde::Serialize;

pub enum Content<MultiPart: Serialize + Send> {
    Text(String),
    Multipart(MultiPart),
}

pub enum TransportError {
    Unknown,
}

pub type TransportResult<T> = Result<T, TransportError>;

#[async_trait]
pub trait Transport {
    async fn send<'s, Body: Serialize + Debug + Send>(
        key: &'s str,
        content: Content<Body>,
    ) -> TransportResult<()>;
}
