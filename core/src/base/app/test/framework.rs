use async_trait::async_trait;

use super::super::types::Aggregate;
use super::executor::AggregateTestExecutor;

/// A framework for rigorously testing the aggregate logic, one of the *most important*
/// parts of any DDD system.
pub struct TestFramework<A: Aggregate> {
    service: A::Services,
}

impl<A: Aggregate> TestFramework<A> {
    /// Create a test framework using the provided service.
    pub fn with(service: A::Services) -> Self {
        Self { service }
    }
}

impl<A> TestFramework<A>
where
    A: Aggregate,
{
    #[must_use]
    pub fn given_no_previous_events(self) -> AggregateTestExecutor<A> {
        AggregateTestExecutor::new(Vec::new(), self.service)
    }

    #[must_use]
    pub fn given(self, events: Vec<A::Event>) -> AggregateTestExecutor<A> {
        AggregateTestExecutor::new(events, self.service)
    }
}
