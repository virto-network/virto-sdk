use super::types::Message;
use serde::Deserialize;

#[derive(Deserialize)]
pub enum WalletCommand {
    AddMessageToSign(Message),
    Sign(),
}
