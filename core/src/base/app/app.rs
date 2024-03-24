use super::types::{
    AppInfo, AppRunnable, DomainEvent, EventEnvelope, SerializedCommandEnvelope, StateMachine,
};

use crate::{prelude::*, CommittedEventEnvelope, ConstructableService, SerializedEventEnvelope};
use crate::{to_serialized_event_envelope, RunnableError};

struct AppBuilder<S, Args>
where
    S: StateMachine,
    S::Services: ConstructableService<Args = Args, Service = S::Services>,
    Args: Clone,
{
    args: Args,
    app_info: AppInfo,
    state_machine: PhantomData<S>,
}

impl<S, Args> AppBuilder<S, Args>
where
    S: StateMachine,
    S::Services: ConstructableService<Args = Args, Service = S::Services>,
    Args: Clone,
{
    fn new(app_info: AppInfo, args: Args) -> Self {
        Self {
            app_info,
            args,
            state_machine: Default::default(),
        }
    }

    fn run(&self, initial_state: Option<Value>) -> impl AppRunnable + '_ {
        let state_machine: S = match (initial_state) {
            Some(value) => serde_json::from_value(value).expect("Can not serialize State"),
            None => S::default(),
        };

        let service = S::Services::new(self.args.clone());

        App::new(&self.app_info, state_machine, service)
    }
}

pub struct App<'a, S>
where
    S: StateMachine,
{
    app_info: &'a AppInfo,
    services: S::Services,
    state_machine: S,
}

impl<'a, S: StateMachine> App<'a, S> {
    fn new(app_info: &'a AppInfo, state: S, services: S::Services) -> Self {
        Self {
            app_info,
            services,
            state_machine: state,
        }
    }
}

impl<'a, S> AppRunnable for App<'a, S>
where
    S: StateMachine,
{
    fn snap_shot(&self) -> Value {
        serde_json::to_value(&self.state_machine).expect("It must be a serializable state_machine")
    }

    fn get_app_info(&self) -> &AppInfo {
        &self.app_info
    }

    async fn apply(&mut self, event: CommittedEventEnvelope) -> Result<(), RunnableError> {
        let event: S::Event =
            serde_json::from_value(event.payload).map_err(|_| RunnableError::Unknown)?;
        println!("{:?}", event);

        self.state_machine.apply(event);
        Ok(())
    }

    async fn run_command(
        &self,
        command: SerializedCommandEnvelope,
    ) -> Result<Vec<SerializedEventEnvelope>, RunnableError> {
        let cmd: S::Command =
            serde_json::from_value(command.cmd_payload).map_err(|_| RunnableError::Unknown)?;

        let events = self
            .state_machine
            .handle(cmd, &self.services)
            .await
            .map_err(|x| RunnableError::Unknown)?;

        Ok(events
            .into_iter()
            .map(|e| {
                to_serialized_event_envelope::<S::Event>(
                    &self.app_info.id,
                    &command.aggregate_id,
                    &e,
                    &command.metadata,
                )
            })
            .collect())
    }
}

mod app_test {
    use std::process::Command;

    use crate::{to_serialized_command_envelope, AppPermission, DomainCommand};
    use async_std::test;

    use super::*;

    #[derive(Deserialize, Serialize, Debug)]
    enum TestCmd {
        A,
        B,
    }

    #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
    enum TestEvent {
        A,
        B(usize),
    }

    #[derive(Debug)]
    enum TestError {
        A,
        B,
    }

    impl core::fmt::Display for TestError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::A => write!(f, "Error::A"),
                Self::B => write!(f, "Error::B"),
            }
        }
    }

    impl core::error::Error for TestError {}

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            match self {
                TestEvent::A => "A".into(),
                TestEvent::B(_) => "B".into(),
            }
        }

        fn event_version(&self) -> String {
            "0.0.1".into()
        }
    }

    impl DomainCommand for TestCmd {
        fn command_name(&self) -> String {
            match self {
                TestCmd::A => "A".into(),
                TestCmd::B => "B".into(),
            }
        }

        fn command_payload(&self) -> Value {
            serde_json::to_value(self).expect("Can not Seralize command")
        }
    }

    #[derive(Serialize, Deserialize)]
    struct TestAggregate {
        pub sum: usize,
    }

    impl Default for TestAggregate {
        fn default() -> Self {
            Self { sum: 0 }
        }
    }

    trait TestService {
        fn sign(&self) -> usize;
    }

    #[derive(Deserialize, Clone)]
    struct ServiceConfig {
        url: String,
    }

    #[derive(Default)]
    struct Service {
        url: String,
    }

    impl Service {
        fn new(args: ServiceConfig) -> Self {
            Service { url: args.url }
        }
    }

    impl TestService for Service {
        fn sign(&self) -> usize {
            10
        }
    }

    fn to_envelop_cmd(cmd: TestCmd) -> SerializedCommandEnvelope {
        to_serialized_command_envelope("0.0.1", "david", cmd, HashMap::new())
    }

    impl ConstructableService for Box<dyn TestService> {
        type Args = ServiceConfig;
        type Service = Box<dyn TestService>;

        fn new(args: Self::Args) -> Self::Service {
            Box::new(Service { url: args.url })
        }
    }

    impl StateMachine for TestAggregate {
        type Command = TestCmd;
        type Event = TestEvent;
        type Error = TestError;
        type Services = Box<dyn TestService>;

        async fn handle(
            &self,
            command: Self::Command,
            ctx: &Self::Services,
        ) -> Result<Vec<Self::Event>, Self::Error> {
            match command {
                TestCmd::A => Ok(vec![TestEvent::A]),
                TestCmd::B => Ok(vec![TestEvent::A, TestEvent::B(ctx.sign())]),
            }
        }

        fn apply(&mut self, event: Self::Event) {
            match event {
                TestEvent::A => {
                    self.sum += 1;
                }
                TestEvent::B(u) => {
                    self.sum += u;
                }
            }
        }
    }

    fn mock_app_info() -> AppInfo {
        AppInfo {
            author: "foo".into(),
            description: "foo".into(),
            id: "foo".into(),
            name: "hello".into(),
            permissions: vec![],
            version: "0.0.1".into(),
        }
    }

    fn to_committed_event(
        sequence: usize,
        event: SerializedEventEnvelope,
    ) -> CommittedEventEnvelope {
        CommittedEventEnvelope {
            aggregate_id: event.aggregate_id,
            app_id: event.app_id,
            event_type: event.event_type,
            event_version: event.event_version,
            metadata: event.metadata,
            payload: event.payload,
            sequence,
        }
    }

    #[async_std::test]
    async fn setup_app() {
        let info = mock_app_info();
        let appSpawner = AppBuilder::<TestAggregate, ServiceConfig>::new(
            info,
            ServiceConfig { url: "http".into() },
        );
        let mut app = appSpawner.run(None);

        let events = app
            .run_command(to_envelop_cmd(TestCmd::A))
            .await
            .expect("hello");

        for (seq, e) in events.into_iter().enumerate() {
            app.apply(to_committed_event(seq, e)).await;
        }

        println!("{:?}", app.snap_shot());
    }
}
