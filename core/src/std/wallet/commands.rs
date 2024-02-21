use super::types::Message;

pub enum WalletCommand {
    AddMessageToSign(Message),
    Sign(),
}
