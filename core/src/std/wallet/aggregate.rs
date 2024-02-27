use std::marker::PhantomData;

use async_trait::async_trait;
use futures::future;
use serde::{Deserialize, Serialize};

use super::commands::WalletCommand;
use super::events::{WalletError, WalletEvent};
use super::services::{WalletApi, WalletResult, WalletServices};
use super::types::Message;
use crate::cqrs::aggregate::Aggregate;

#[derive(Deserialize, Serialize, Debug)]
pub struct Wallet<S> {
    device_id: Option<String>,
    pending_messages: Vec<Message>,
    #[serde(skip_serializing)]
    phantom: PhantomData<S>,
}

#[async_trait]
impl<S: WalletApi + Sync + Send> Aggregate for Wallet<S> {
    type Command = WalletCommand;
    type Event = WalletEvent<<S as WalletApi>::SignedPayload>;
    type Error = WalletError;
    type Services = WalletServices<S>;

    fn aggregate_type() -> String {
        "wallet_service".to_string()
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            WalletEvent::Signed(..) => {
                self.pending_messages = vec![];
            }
            WalletEvent::AddedMessageToSign(message) => {
                self.pending_messages.push(message);
            }
        }
    }

    async fn handle(
        &self,
        cmd: Self::Command,
        svc: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match cmd {
            WalletCommand::AddMessageToSign(message) => {
                Ok(vec![WalletEvent::AddedMessageToSign(message)])
            }
            WalletCommand::Sign() => {
                if self.pending_messages.len() == 0 {
                    return Err(WalletError::NoMesssagesToSign);
                }

                let messagess_signatures =
                    future::try_join_all(self.pending_messages.iter().map(|m: &Message| async {
                        Ok((
                            svc.services
                                .sign(m)
                                .await
                                .map_err(|_| WalletError::Unknown)?,
                            m.to_owned(),
                        ))
                    }))
                    .await?;

                Ok(vec![WalletEvent::Signed(messagess_signatures)])
            }
        }
    }
}

impl<T> Default for Wallet<T> {
    fn default() -> Self {
        Self {
            device_id: None,
            pending_messages: vec![],
            phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod wallet_test {
    use super::super::services::*;
    use super::*;
    use crate::cqrs::test::TestFramework;
    use async_std;
    use async_trait;

    type WalletService = TestFramework<Wallet<MockWalletService>>;

    #[test]
    fn add_pending_tx() {
        let message = Message::try_from([0u8, 1u8, 2u8].as_slice()).expect("can not convert");

        WalletService::with(WalletServices::new(MockWalletService::default()))
            .given_no_previous_events()
            .when(WalletCommand::AddMessageToSign(message.clone()))
            .then_expect_events(vec![WalletEvent::AddedMessageToSign(message)]);
    }

    #[async_std::test]
    async fn no_messsages_to_sign() {
        let message = Message::try_from([0u8, 1u8, 2u8].as_slice()).expect("can not convert");

        let executor = WalletService::with(WalletServices::new(MockWalletService::default()))
            .given_no_previous_events()
            .when(WalletCommand::Sign())
            .then_expect_error(WalletError::NoMesssagesToSign);
    }

    #[async_std::test]
    async fn sign_tx_queues() {
        let message = Message::try_from([0u8, 1u8, 2u8].as_slice()).expect("can not convert");

        let executor = WalletService::with(WalletServices::new(MockWalletService::default()))
            .given(vec![WalletEvent::AddedMessageToSign(message.clone())]);

        let validator = executor.when_async(WalletCommand::Sign()).await;
        validator.then_expect_events(vec![WalletEvent::Signed(vec![(
            Message::try_from([1u8].as_slice()).expect("can not convert"),
            message,
        )])])
    }

    #[derive(Default)]
    struct MockWalletService;

    #[async_trait]
    impl WalletApi for MockWalletService {
        type SignedPayload = Message;
        async fn sign<'p>(&self, payload: &'p [u8]) -> WalletResult<Self::SignedPayload> {
            Ok(Message::try_from([1u8].as_slice()).expect("hello world"))
        }
    }
}
