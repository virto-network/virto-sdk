use crate::utils::HashMap;
use async_trait::async_trait;

use super::{Aggregate, AggregateError, EventEnvelope};

#[async_trait]
pub trait EventStore<A>: Send + Sync
where
    A: Aggregate,
{
    type AC: AggregateContext<A> + Sync + Send;

    async fn load_events(
        &self,
        aggregate_id: &str,
    ) -> Result<Vec<EventEnvelope<A::Event>>, AggregateError<A::Error>>;

    async fn load_aggregate(
        &self,
        aggregate_id: &str,
    ) -> Result<Self::AC, AggregateError<A::Error>>;

    async fn commit(
        &self,
        events: Vec<A::Event>,
        context: Self::AC,
        metadata: HashMap<String, String>,
    ) -> Result<Vec<EventEnvelope<A::Event>>, AggregateError<A::Error>>;
}

pub trait AggregateContext<A>
where
    A: Aggregate,
{
    fn aggregate(&self) -> &A;
}
