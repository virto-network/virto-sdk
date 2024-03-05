use super::types::{Aggregate, DomainEvent, EventEnvelope};
use crate::utils::prelude::*;

pub trait Query<Event>: Send + Sync
where
    Event: DomainEvent,
{
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Event>]);
}

