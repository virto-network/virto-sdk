use super::cqrs::Query;
use crate::{
    cqrs::{event, DomainEvent},
    utils,
};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

pub enum VRunnerError {
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedEvent {
    pub app_id: String,
    pub aggregate_id: String,
    pub sequence: usize,
    pub event_type: String,
    pub event_version: String,
    pub payload: Value,
    pub metadata: Value,
}

#[async_trait]
pub trait VQuery: Sync + Send {
    async fn dispatch(&self, aggregate_id: &str, events: &[SerializedEvent]);
}

#[async_trait] // TODO: Remove async_trait
pub trait VRunnable {
    async fn setup(mut self, queries: Vec<Box<dyn VQuery>>) -> Result<(), VRunnerError>;

    async fn exec<'a>(
        &self,
        aggregaate_id: &'a str,
        command: Value,
        metadata: utils::HashMap<String, String>,
    ) -> Result<(), VRunnerError>;
}

#[derive(Serialize, Deserialize)]
struct CommandEvelope
{
    to: String,   // app-id
    from: String, // app-id
    aggregate_id: String,
    command: Value,
    sequence: u64,
    metadata: utils::HashMap<String, String>, // { who, req_id }
}

#[async_trait]
pub trait VSupervisor {
    fn add(app_id: &str, app: Box<dyn VRunnable>);
    fn run(cmd: CommandEvelope<Command>);
}
