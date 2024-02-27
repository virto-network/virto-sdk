use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::event::DomainEvent;

#[async_trait]
pub trait Aggregate: Default + Serialize + DeserializeOwned + Sync + Send {
    type Command: DeserializeOwned + Sync + Send;

    type Event: DomainEvent;

    type Error: std::error::Error;

    type Services: Send + Sync;
    fn aggregate_type() -> String;

    async fn handle(
        &self,
        command: Self::Command,
        service: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error>;

    fn apply(&mut self, event: Self::Event);
}
