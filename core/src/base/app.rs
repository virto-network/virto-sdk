use std::default;
use std::marker::PhantomData;

use async_trait::async_trait;

use super::runner::{SerializedEvent, VRunnable, VRunnerError};
use super::VAppInfo;
use crate::cqrs::{
    Aggregate, AggregateContext, CqrsFramework, DomainEvent, EventEnvelope, EventStore, Query,
};
use crate::VQuery;
use crate::{std::wallet::aggregate, utils};
use futures::executor;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AppPermission {
    name: String,
    description: String,
    app: String, // app
    cmds: Vec<String>,
    events: Vec<String>,
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
enum VIPCError {
    Unknown,
}

pub trait VIPC<A> {
    fn main_service() -> A;
    fn exec(cmd: String, value: serde_json::Value) -> Result<(), VIPCError>;
}

pub trait VAggregate:
    Aggregate<Services = <Self as VAggregate>::Services, Command = <Self as VAggregate>::Command>
{
    type Services: Sync + Send;
    type Command: DeserializeOwned;
}

#[derive(Debug)]
pub enum EventFactoyError {}

pub trait VEventStoreFactory {
    fn build<'a, A: Aggregate>(&self, id: &'a str) -> Result<impl EventStore<A>, EventFactoyError>;
}

#[derive(Default)]
pub struct VAppBuilder<A: Aggregate, AC: AggregateContext<A>> {
    queries: Option<Vec<Box<dyn Query<A::Event>>>>,
    store: Option<Box<dyn EventStore<A, AC = AC>>>,
    services: Option<Box<A::Services>>,
    app_info: Option<AppInfo>,
}

impl<A: Aggregate, AC: AggregateContext<A>> VAppBuilder<A, AC> {
    fn with_app_info(self, app_info: AppInfo) -> Self {
        Self {
            app_info: Some(app_info),
            ..self
        }
    }

    fn with_services(self, services: Box<A::Services>) -> Self {
        Self {
            services: Some(services),
            ..self
        }
    }

    fn with_queries(self, queries: Vec<Box<dyn Query<A::Event>>>) -> Self {
        Self {
            queries: Some(queries),
            ..self
        }
    }

    fn with_store(self, store: Box<dyn EventStore<A, AC = AC>>) -> Self {
        Self {
            store: Some(store),
            ..self
        }
    }
}

pub struct VApp<A: Aggregate, E: EventStore<A>> {
    app_info: AppInfo,
    cqrs: CqrsFramework<A, E>,
    is_setup: bool,
}

impl<A: Aggregate, E: EventStore<A>> VApp<A, E> {
    // todo create new signature without empty queries
    pub fn new(
        app_info: AppInfo,
        event_store: E,
        queries: Vec<Box<dyn Query<A::Event>>>,
        services: A::Services,
    ) -> Self {
        Self {
            app_info,
            cqrs: CqrsFramework::new(event_store, queries, services),
            is_setup: false,
        }
    }
}

fn to_serialized_event<Event: DomainEvent>(
    app_id: impl Into<String>,
    event: &EventEnvelope<Event>,
) -> SerializedEvent {
    SerializedEvent {
        aggregate_id: event.aggregate_id.clone(),
        sequence: event.sequence.clone(),
        payload: serde_json::to_value(event.payload.clone()).expect("Error deserializing Value"),
        metadata: serde_json::to_value(event.metadata.clone()).expect("Error deserializing Value"),
        event_type: event.payload.event_type(),
        event_version: event.payload.event_version(),
        app_id: app_id.into(),
    }
}

pub struct QueryBridge<E>
where
    E: DomainEvent,
{
    inner: Box<dyn VQuery>,
    phantom: PhantomData<E>,
    app_id: String,
}

impl<E> QueryBridge<E>
where
    E: DomainEvent,
{
    fn new(app_id: impl Into<String>, inner: Box<dyn VQuery>) -> Self {
        Self {
            inner,
            phantom: PhantomData,
            app_id: app_id.into(),
        }
    }
}

#[async_trait]
impl<E> Query<E> for QueryBridge<E>
where
    E: DomainEvent,
{
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<E>]) {
        let events: Vec<SerializedEvent> = events
            .iter()
            .map(|x| to_serialized_event(&self.app_id, x.into()))
            .collect();
        self.inner.dispatch(aggregate_id, &events);
    }
}

#[async_trait]
impl<A: Aggregate + 'static, E: EventStore<A>> VRunnable for VApp<A, E> {
    async fn setup(mut self, queries: Vec<Box<dyn VQuery>>) -> Result<(), VRunnerError> {
        if self.is_setup {
            return Ok(());
        }

        for q in queries {
            self.cqrs = self
                .cqrs
                .append_query(Box::new(QueryBridge::new(&self.app_info.id, q)));
        }

        self.is_setup = true;

        Ok(())
    }

    async fn exec<'a>(
        &self,
        aggregate_id: &'a str,
        command: serde_json::Value,
        metadata: utils::HashMap<String, String>,
    ) -> Result<(), VRunnerError> {
        let command: A::Command =
            serde_json::from_value(command).map_err(|_| VRunnerError::Unknown)?;

        self.cqrs
            .execute_with_metadata(aggregate_id, command, metadata)
            .await
            .map_err(|_| VRunnerError::Unknown)
    }
}

impl<A: Aggregate, E: EventStore<A>> VAppInfo for VApp<A, E> {
    fn get_app_info(&self) -> &AppInfo {
        &self.app_info
    }
}
