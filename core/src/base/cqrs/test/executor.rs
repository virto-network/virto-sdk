use super::super::Aggregate;
use super::validator::AggregateResultValidator;
use async_trait::async_trait;
use tokio::main;

pub struct AggregateTestExecutor<A>
where
    A: Aggregate,
{
    events: Vec<A::Event>,
    service: A::Services,
}

impl<A> AggregateTestExecutor<A>
where
    A: Aggregate,
{
    pub fn when(self, command: A::Command) -> AggregateResultValidator<A> {
        let result = when::<A>(self.events, command, self.service);
        AggregateResultValidator::new(result)
    }

    pub async fn when_async(self, command: A::Command) -> AggregateResultValidator<A> {
        let mut aggregate = A::default();
        for event in self.events {
            aggregate.apply(event);
        }
        let result = aggregate.handle(command, &self.service).await;
        AggregateResultValidator::new(result)
    }

    #[must_use]
    pub fn and(self, new_events: Vec<A::Event>) -> Self {
        let mut events = self.events;
        events.extend(new_events);
        let service = self.service;
        AggregateTestExecutor { events, service }
    }

    pub(crate) fn new(events: Vec<A::Event>, service: A::Services) -> Self {
        Self { events, service }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn when<A: Aggregate>(
    events: Vec<A::Event>,
    command: A::Command,
    service: A::Services,
) -> Result<Vec<A::Event>, A::Error> {
    let mut aggregate = A::default();
    for event in events {
        aggregate.apply(event);
    }
    aggregate.handle(command, &service).await
}
