use crate::utils::prelude::*;
pub trait DomainEvent:
    Serialize + DeserializeOwned + Clone + Debug + Send
{
    fn event_type(&self) -> String;

    fn event_version(&self) -> String;
}


pub trait Aggregate: Default + Serialize + DeserializeOwned + Send {
    type Command: DeserializeOwned + Send + Debug + Serialize;

    type Event: DomainEvent + Send + PartialEq;

    type Error: core::error::Error;

    type Services;

    async fn handle(
        &self,
        command: Self::Command,
        service: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error>; // chante IntoInter

    fn apply(&mut self, event: Self::Event);
}


#[derive(Serialize, Deserialize)]
pub struct CommandSerializedEvelope {
    pub app_id: String, // app-id
    pub aggregate_id: String,
    pub metadata: HashMap<String, String>, // { who, req_id }
    pub cmd_name: String,
    pub cmd_payload: Value,
}


#[derive(Debug)]
pub struct EventEnvelope<E>
where
    E: DomainEvent,
{
    /// The id of the aggregate instance.
    pub aggregate_id: String,
    /// The sequence number for an aggregate instance.
    pub sequence: usize,
    /// The event payload with all business information.
    pub payload: E,
    /// Additional metadata for use in auditing, logging or debugging purposes.
    pub metadata: HashMap<String, String>,
}

impl<E: DomainEvent> Clone for EventEnvelope<E> {
    fn clone(&self) -> Self {
        Self {
            aggregate_id: self.aggregate_id.clone(),
            sequence: self.sequence,
            payload: self.payload.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventEvelope {
    pub app_id: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub event_version: String,
    pub payload: Value,
    pub metadata: Value,
}


#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventCommitedEvelope {
    pub app_id: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub event_version: String,
    pub sequence: usize, // after commit we know the squence for that event
    pub payload: Value,
    pub metadata: Value,
}

pub fn to_serialized_event_evelope<E: DomainEvent>(
    app_id: impl Into<String>,
    aggregate_id: impl Into<String>,
    event: &E,
    metadata: &HashMap<String, String>,
) -> EventEvelope {
    EventEvelope {
        app_id: app_id.into(),
        aggregate_id: aggregate_id.into(),
        metadata: serde_json::to_value(metadata).expect("invalid metadata"), // exploted
        payload: serde_json::to_value(event).expect("invalid metadata"), // exploted
        event_type: event.event_type(),
        event_version: event.event_version(),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AppPermission {
    pub name: String,
    pub description: String,
    pub app: String, // app
    pub cmds: Vec<String>,
    pub events: Vec<String>,
}


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub permissions: Vec<AppPermission>,
}

#[derive(Debug)]
pub enum RunnableError {
    Unknown,
}

pub trait AppRunnable {
    fn get_app_info(&self) -> &AppInfo;

    async fn apply(&mut self, event: EventCommitedEvelope) -> Result<(), RunnableError>;

    async fn run_command(&self, command: CommandSerializedEvelope)
        -> Result<Vec<EventEvelope>, RunnableError>;
}

