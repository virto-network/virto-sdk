use async_trait::async_trait;
use core::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::aggregate::Aggregate;
use super::event::{DomainEvent, EventEnvelope};

#[async_trait]
pub trait Query<Event>: Send + Sync
where
    Event: DomainEvent,
{
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Event>]);
}

pub trait View<Event>: Debug + Default + Serialize + DeserializeOwned + Send + Sync
where
    Event: DomainEvent,
{
    fn update(&mut self, event: &EventEnvelope<Event>);
}
