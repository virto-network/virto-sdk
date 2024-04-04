use super::types::{
    AppInfo, AppRunnable, DomainEvent, EventEnvelope, SerializedCommandEnvelope, StateMachine,
};

use crate::{prelude::*, CommittedEventEnvelope, ConstructableService, SerializedEventEnvelope};
use crate::{to_serialized_event_envelope, RunnableError};

pub struct AppFactory<S, Args>
where
    S: StateMachine + Sync + Send,
    S::Services: ConstructableService<Args = Args, Service = S::Services>,
    Args: Clone,
{
    args: Args,
    app_info: AppInfo,
    state_machine: PhantomData<S>,
}

pub trait AppLoader<'r>: Send + Sync {
    fn app_info(&self) -> &AppInfo;
    fn run(&self, state: Option<Value>) -> Box<dyn AppRunnable + 'r>;
}

impl<S, Args> AppFactory<S, Args>
where
    S: StateMachine + Sync + Send ,
    S::Services: ConstructableService<Args = Args, Service = S::Services>,
    Args: Clone + Send + Sync,
{
    pub fn new(app_info: AppInfo, args: Args) -> Self {
        Self {
            app_info,
            args,
            state_machine: Default::default(),
        }
    }
}

impl<'r, S, Args> AppLoader<'r> for AppFactory<S, Args>
where
    S: StateMachine + Sync + Send + 'r,
    S::Services: ConstructableService<Args = Args, Service = S::Services>,
    Args: Clone + Send + Sync,
{
    fn app_info(&self) -> &AppInfo {
        &self.app_info
    }

    fn run(&self, state: Option<Value>) -> Box<dyn AppRunnable + 'r> {
        let state_machine: S = match state {
            Some(value) => serde_json::from_value(value).expect("Can not serialize State"),
            None => S::default(),
        };

        let service = S::Services::new(self.args.clone());

        Box::new(App::new(self.app_info.clone(), state_machine, service))
    }
}

pub struct App<S>
where
    S: StateMachine + Send + Sync,
{
    app_info: AppInfo,
    services: S::Services,
    state_machine: S,
}

impl<S> App<S> where S:  StateMachine + Send + Sync {
    fn new(app_info: AppInfo, state: S, services: S::Services) -> Self {
        Self {
            app_info,
            services,
            state_machine: state,
        }
    }
}

#[async_trait]
impl<S> AppRunnable for App<S>
where
    S: StateMachine + Send + Sync,
{
    fn snapshot(&self) -> Value {
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
            .map_err(|_| RunnableError::Unknown)?;

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
            serde_json::to_value(self).expect("Can not Serialize command")
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

    trait TestService: Sync + Send {
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
            Box::new(Service::new(args))
        }
    }

    #[async_trait]
    impl StateMachine for TestAggregate {
        type Command = TestCmd;
        type Event = TestEvent;
        type Error = TestError;
        type Services = Box<dyn TestService + 'static>;

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
    async fn run_command_and_take_snapshot() {
        let info = mock_app_info();

        let appSpawner = AppFactory::<TestAggregate, ServiceConfig>::new(
            info,
            ServiceConfig { url: "http".into() }, // container reference
        );

        let mut app = appSpawner.run(None);

        let events = app
            .run_command(to_envelop_cmd(TestCmd::B))
            .await
            .expect("hello");

        for (seq, e) in events.into_iter().enumerate() {
            app.apply(to_committed_event(seq, e)).await;
        }

        let snapshot = app.snapshot();
        let state: TestAggregate = serde_json::from_value(snapshot).expect("It must increase");
        assert!(state.sum == 11);
    }
}
