use core::marker::PhantomData;

#[cfg(feature = "no_std")]
use alloc::sync::Arc;

#[cfg(not(feature = "no_std"))]
use std::rc::Rc;

use crate::{
    AppInfo, AppLoader, AppRunnable, CommittedEventEnvelope, SerializedCommandEnvelope,
    SerializedEventEnvelope,
};
use async_std::stream::StreamExt;
use async_std::task::spawn;

use async_trait::async_trait;
use futures::channel::mpsc::{channel, Receiver, Sender};
use futures::stream::Next;
use futures::{Future, SinkExt, Stream};
use serde_json::Value;

#[derive(Debug)]
enum ProcessError {
    Unknown,
}

pub type ProcessResult<A> = Result<A, ProcessError>;

#[derive(Debug)]
enum Instruction {
    Cmd(SerializedCommandEnvelope),
    CommitEvent(CommittedEventEnvelope),
    Snapshot,
    Kill,
}

#[derive(Debug)]
enum Response {
    Event(SerializedEventEnvelope),
    Snapshot((String, Value)), // id , state
    Exit
}

#[async_trait]
pub trait Process {
    fn spawn<'r: 'static>(state: Option<Value>, loader: &Box<dyn AppLoader<'r>>) -> Self;

    async fn write(&mut self, instruction: Instruction) -> ProcessResult<()>;
    async fn next(&mut self) -> Option<Response>;
}

#[cfg(feature = "single_thread")]
struct AppProcess {
    pub tx: Sender<Instruction>,
    rx_response: Receiver<Response>,
}

#[async_trait]
impl Process for AppProcess {
    fn spawn<'r: 'static>(state: Option<Value>, loader: &Box<dyn AppLoader<'r>>) -> Self {
        let (tx, mut rx) = channel::<Instruction>(100);
        let (mut tx_response, rx_response) = channel::<Response>(100);
        let mut app = loader.run(state);

        spawn(async move {
            while let Some(instruction) = rx.next().await {
                match instruction {
                    Instruction::Cmd(command) => {
                        let events = app
                            .run_command(command)
                            .await
                            .map_err(|_| ProcessError::Unknown)?;

                        for i in events {
                            tx_response
                                .send(Response::Event(i))
                                .await
                                .map_err(|_| ProcessError::Unknown)?;
                        }
                    }
                    Instruction::CommitEvent(event) => {
                        app.apply(event).await.map_err(|_| ProcessError::Unknown)?;
                    }
                    Instruction::Snapshot => {
                        let snapshot = app.snapshot();

                        tx_response
                            .send(Response::Snapshot((
                                app.get_app_info().id.to_string(),
                                snapshot,
                            )))
                            .await
                            .map_err(|_| ProcessError::Unknown)?;
                    }
                    Instruction::Kill => {
                        drop(app);
                        tx_response.send(Response::Exit);
                        return Err(ProcessError::Unknown);
                    }
                }
            }

            Ok::<(), ProcessError>(())
        });

        Self { tx, rx_response }
    }

    async fn write(&mut self, inst: Instruction) -> ProcessResult<()> {
        println!("send it {:?}", &inst);
        self.tx
            .send(inst)
            .await
            .map_err(|_| ProcessError::Unknown)?;
        Ok(())
    }

    async fn next(&mut self) -> Option<Response> {
        self.rx_response.next().await
    }
}

mod process_test {

    use crate::{to_serialized_command_envelope, AppPermission, DomainCommand};
    use async_std::test;

    use super::super::super::app::*;
    use super::*;
    use crate::prelude::*;
    use async_trait::async_trait;
    use futures::channel::mpsc::{channel, Receiver, Sender};
    use std::sync::Arc;

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

    trait TestService: Sync {
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

    impl ConstructableService for Box<dyn TestService + Sync + Send> {
        type Args = ServiceConfig;
        type Service = Box<dyn TestService + Sync + Send>;

        fn new(args: Self::Args) -> Self::Service {
            Box::new(Service::new(args))
        }
    }

    #[async_trait]
    impl StateMachine for TestAggregate {
        type Command = TestCmd;
        type Event = TestEvent;
        type Error = TestError;
        type Services = Box<dyn TestService + Send + Sync>;

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
    async fn spawn_process() {
        let info = mock_app_info();

        let appLoader: Box<dyn AppLoader> =
            Box::new(AppFactory::<TestAggregate, ServiceConfig>::new(
                info,
                ServiceConfig { url: "http".into() },
            ));

        let mut process = AppProcess::spawn(None, &appLoader);

        process
            .write(Instruction::Cmd(to_envelop_cmd(TestCmd::A)))
            .await;

        process
            .write(Instruction::Cmd(to_envelop_cmd(TestCmd::A)))
            .await;

        process
            .write(Instruction::Cmd(to_envelop_cmd(TestCmd::A)))
            .await;

        process.write(Instruction::Snapshot).await;

        process.write(Instruction::Kill).await;

        process.write(Instruction::Snapshot).await;
        process
            .write(Instruction::Cmd(to_envelop_cmd(TestCmd::A)))
            .await;

        while let Some(event) = process.next().await {
            println!("Response={:?}", event);
        }
    }
}
