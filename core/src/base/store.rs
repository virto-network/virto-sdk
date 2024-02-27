use crate::{app::AppInfo, SDKCore};
use async_trait::async_trait;
use cqrs_es::{Aggregate, AggregateContext, AggregateError, EventEnvelope, EventStore};
use matrix_sdk::Client;
use std::marker::PhantomData;
use crate::utils::HashMap;

struct MatrixEventStore<A: Aggregate> {
    inner: Box<SDKCore>,
    app_info: AppInfo,
    phantom: PhantomData<A>,
}

impl<A: Aggregate> MatrixEventStore<A> {
    pub(crate) fn new(inner: Box<SDKCore>, app_info: AppInfo) -> Self {
        Self {
            inner,
            app_info,
            phantom: PhantomData,
        }
    }
}

pub struct MatrixContext<A: Aggregate> {
    pub aggregate: A,
    pub aggregate_id: String,
    pub current_sequence: usize,
}

impl<A> AggregateContext<A> for MatrixContext<A>
where
    A: Aggregate,
{
    fn aggregate(&self) -> &A {
        &self.aggregate
    }
}

#[async_trait]
impl<A> EventStore<A> for MatrixEventStore<A>
where
    A: Aggregate,
{
    type AC = MatrixContext<A>;

    async fn load_events(
        &self,
        aggregate_id: &str,
    ) -> Result<Vec<EventEnvelope<A>>, AggregateError<A::Error>> {

        todo!();
    }

    async fn load_aggregate(
        &self,
        aggregate_id: &str,
    ) -> Result<Self::AC, AggregateError<A::Error>> {
        let aggregate = A::default();
        // cargar el aggregate
        for event in self.load_events(aggregate_id).await? {
            aggregate.apply(event)
        }

        todo!()
    }

    async fn commit(
        &self,
        events: Vec<A::Event>,
        context: Self::AC,
        metadata: HashMap<String, String>,
    ) -> Result<Vec<EventEnvelope<A>>, AggregateError<A::Error>> {
        // escirbir en el room el evento
        todo!()
    }
}






