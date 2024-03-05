use super::types::{
    Aggregate, AppInfo, AppRunnable, CbEventEmmiter, CommandSerializedEvelope,
    EventEvelope, DomainEvent
};

use crate::{prelude::*, EventCommitedEvelope};
use crate::{
    to_serialized_event_evelope,
    RunnableError
};

pub struct App<A>
where
    A: Aggregate,
{
    app_info: AppInfo,
    services: A::Services,
    aggregate: A
}

impl<A> App where A: Aggregate {
    // fn create(info: AppInfo)
}

impl<A> AppRunnable for App<A>
where
    A: Aggregate,
{

    fn get_app_info(&self) -> &AppInfo {
        &self.app_info
    }

    async fn apply(&mut self, event: EventCommitedEvelope) -> Result<(), RunnableError> {
        let event: A::Event = serde_json::from_value(event.payload).map_err(|_| RunnableError::Unknown)?;
        self.aggregate_mut().apply(event);
        Ok(())
    }

    async fn run_command(
        &self,
        command: CommandSerializedEvelope,
    ) -> Result<(), RunnableError> {
        let mut cmd_obj: Map<String, Value> = Map::new();
        cmd_obj.insert(command.cmd_name.to_string(), command.cmd_payload);
        println!("OBJ {:?}", cmd_obj);
        let cmd: A::Command = serde_json::from_value(Value::Object(cmd_obj)).map_err(|_| RunnableError::Unknown)?;
        println!("DONE {:?}", cmd);
        let events = self
            .aggregate()
            .handle(cmd, &self.services)
            .await
            .map_err(|x| RunnableError::Unknown)?;


        let serialized_events: Vec<EventEvelope> = events
            .iter()
            .map(|e| {
                to_serialized_event_evelope::<A::Event>(
                    &self.app_info.id,
                    &command.aggregate_id,
                    &e,
                    &command.metadata,
                )
            })
            .collect();

        Ok(serialized_events)
    }
}


mod app_test {
    use std::process::Command;

    use async_std::test;
    use crate::AppPermission;

    use super::*;


    #[derive(Deserialize, Serialize, Debug)]
    enum TestCmd {
        A,
        B
    }

    #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
    enum TestEvent {
        A,
        B(usize)
    }


    #[derive(Debug)]
    enum TestError {
        A,
        B
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
                TestEvent::B(_) => "B".into()
            }
        }

        fn event_version(&self) -> String {
            "0.0.1".into()
        }
    }

    #[derive(Serialize, Deserialize)]
    struct TestAggregate {
        pub sum: usize
    }

    impl Default for TestAggregate {
        fn default() -> Self {
            Self {
                sum: 0
            }
        }
    }

    trait TestService {
        #[virto(Http)]
        fn sign(&self) -> usize;
    }

    #[derive(Default)]
    struct Service {}

    impl TestService for Service {
        fn sign(&self) -> usize {
            10
        }
    }

    fn to_evelop_cmd() -> CommandSerializedEvelope {
        CommandSerializedEvelope {
            aggregate_id: "test".into(),
            app_id: "app_id".into(),
            cmd_name: "A".into(),
            cmd_payload: Value::Object(Map::new()),
            metadata: HashMap::new()
        }
    }

    impl Aggregate for TestAggregate {
        type Command = TestCmd;
        type Event = TestEvent;
        type Error = TestError;
        type Services = Box<dyn TestService>;

        async fn handle(
                &self,
                command: Self::Command,
                ctx: &Self::Services,
            ) -> Result<impl IntoIter<Self::Event>, Self::Error> {

            match command {
                TestCmd::A => Ok(vec![
                    TestEvent::A,
                ]),
                TestCmd::B => Ok(vec![
                    TestEvent::A,
                    TestEvent::B(service.sign()),
                ])
            }

        }

        // COMMIT 

        fn apply(&mut self, event: Self::Event) {
            match event {
                TestEvent::A => {
                    self.sum+=1;
                },
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
            name: "helloo".into(),
            permissions: vec![],
            version: "0.0.1".into()
        }
    }

    #[async_std::test]
    async fn setup_app() {
        let wallet = TestAggregate::default();
        let info = mock_app_info();
        let mut app = App::<TestAggregate>::new(info, Box::new(Service::default()));
    }
}