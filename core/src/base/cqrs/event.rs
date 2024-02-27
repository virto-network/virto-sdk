use crate::utils::HashMap;
use std::fmt;

use serde::de::DeserializeOwned;
use serde::Serialize;

pub trait DomainEvent:
    Serialize + DeserializeOwned + Clone + PartialEq + fmt::Debug + Sync + Send
{
    fn event_type(&self) -> String;

    fn event_version(&self) -> String;
}

#[derive(Debug)]
pub struct EventEnvelope<Event>
where
    Event: DomainEvent,
{
    pub aggregate_id: String,

    pub sequence: usize,

    pub payload: Event,

    pub metadata: HashMap<String, String>,
}

impl<Event> Clone for EventEnvelope<Event>
where
    Event: DomainEvent,
{
    fn clone(&self) -> Self {
        Self {
            aggregate_id: self.aggregate_id.clone(),
            sequence: self.sequence,
            payload: self.payload.clone(),
            metadata: self.metadata.clone(),
        }
    }
}
