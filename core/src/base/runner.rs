use super::cqrs::Query;
use crate::{
    cqrs::{event, DomainEvent},
    utils,
};
use async_trait::async_trait;
use serde_json::Value;

pub enum VRunnerError {
    Unknown,
}

#[async_trait]
pub trait VRunner<To>
where
    To: Sync + Send,
{
    async fn setup<E: DomainEvent + From<To> + 'static>(
        mut self,
        queries: Vec<Box<dyn Query<E>>>,
    ) -> Result<(), VRunnerError>;

    async fn exec<'a>(
        &self,
        aggregaate_id: &'a str,
        command: Value,
        metadata: utils::HashMap<String, String>,
    ) -> Result<(), VRunnerError>;
}
