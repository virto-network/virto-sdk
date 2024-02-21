use super::services::WalletApiSignedPayloadBounds;
use super::types::Message;

use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize, PartialEq, Debug)]
pub enum WalletEvent<Signature: Sync + Send + Serialize> {
    AddedMessageToSign(Message),
    Signed(Vec<(Signature, Message)>),
}

#[derive(Deserialize, Clone, Debug)]
pub enum WalletError {
    Unknown,
}
impl core::fmt::Display for WalletError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}
impl core::error::Error for WalletError {}

impl<S: WalletApiSignedPayloadBounds> DomainEvent for WalletEvent<S> {
    fn event_type(&self) -> String {
        match self {
            Self::AddedMessageToSign(_) => "UpdatedPendingToSignMessages".to_string(),
            Self::Signed(..) => "Signed".to_string(),
        }
    }
    fn event_version(&self) -> String {
        "0.1.0".to_string()
    }
}
