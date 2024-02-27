use super::services::WalletApiSignedPayloadBounds;
use super::types::Message;

use crate::cqrs::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize, PartialEq, Debug)]
pub enum WalletEvent<Signature: Sync + Send + Serialize> {
    AddedMessageToSign(Message),
    Signed(Vec<(Signature, Message)>),
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub enum WalletError {
    Unknown,
    NoMesssagesToSign,
}

impl core::fmt::Display for WalletError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "Unknown"),
            Self::NoMesssagesToSign => write!(f, "NoMesssagesToSign"),
        }
    }
}

impl core::error::Error for WalletError {}

impl<S: WalletApiSignedPayloadBounds> DomainEvent for WalletEvent<S> {
    fn event_type(&self) -> String {
        match self {
            Self::AddedMessageToSign(_) => "AddedMessageToSign".to_string(),
            Self::Signed(..) => "Signed".to_string(),
        }
    }
    fn event_version(&self) -> String {
        "0.1.0".to_string()
    }
}
