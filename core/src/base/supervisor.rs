use super::cqrs::Query;
use crate::{
    cqrs::{event, DomainEvent},
    utils::{self, HashMap},
    VAppsState,
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
pub trait VRunnable: Sync + Send {
    async fn add_listeners(mut self, queries: Vec<Box<dyn VQuery>>) -> Result<(), VRunnerError>;

    async fn exec<'a>(
        &self,
        aggregaate_id: &'a str,
        command: Value,
        metadata: utils::HashMap<String, String>,
    ) -> Result<(), VRunnerError>;
}

#[derive(Serialize, Deserialize)]
struct CommandEvelope {
    to: String,   // app-id
    from: String, // app-id
    aggregate_id: String,
    sequence: u64,
    metadata: HashMap<String, String>, // { who, req_id }
    cmd_name: String,
    cmd_payload: Value,
}

pub struct VListener {
    pub from_app_id: String,
    pub event: String,
    pub handler: Box<dyn VQuery>,
}

impl VListener {
    fn new(from: impl Into<String>, event: impl Into<String>, handler: Box<dyn VQuery>) -> Self {
        Self {
            from_app_id: from.into(),
            event: event.into(),
            handler,
        }
    }
}

pub enum VSupervisorError {
    Unknown,
    Forbbiden,
}

#[async_trait]
pub trait VSupervisor {
    fn register_runner(
        &mut self,
        app_id: impl Into<String>,
        runner: Box<dyn VRunnable>,
    ) -> Result<(), VSupervisorError>;
    fn register_listener(
        &mut self,
        app_id: impl Into<String>,
        listener: VListener,
    ) -> Result<(), VSupervisorError>;

    async fn exec(&self, cmd: CommandEvelope) -> Result<(), VSupervisorError>;
}

struct SingleThreadSuperVisor {
    apps_state: VAppsState,
    runners: HashMap<String, Box<dyn VRunnable>>,
    listeners: HashMap<String, Vec<VListener>>,
}

impl SingleThreadSuperVisor {
    fn new(apps_state: VAppsState) -> Self {
        Self {
            apps_state,
            runners: HashMap::new(),
            listeners: HashMap::new(),
        }
    }

    fn check_permission_to_listen(&self, app_id: &str, from_app_id: &str, event: &str) -> bool {
        if let Some(metadata) = self.apps_state.get(app_id) {
            if let Some(permission) = metadata
                .app_info
                .permissions
                .iter()
                .find(|p| p.app == from_app_id)
            {
                return permission.events.iter().any(|p| p == event);
            }
        }
        false
    }

    fn check_permission_cmd(&self, app_id: &str, from_app_id: &str, cmd: &str) -> bool {
        if let Some(metadata) = self.apps_state.get(app_id) {
            if let Some(permission) = metadata
                .app_info
                .permissions
                .iter()
                .find(|p| p.app == from_app_id)
            {
                return permission.cmds.iter().any(|p| p == cmd);
            }
        }
        false
    }
}

#[async_trait]
impl VSupervisor for SingleThreadSuperVisor {
    fn register_listener(
        &mut self,
        app_id: impl Into<String>,
        listener: VListener,
    ) -> Result<(), VSupervisorError> {
        let app_id: String = app_id.into();

        if !self.check_permission_to_listen(&app_id, &listener.from_app_id, &listener.event) {
            return Err(VSupervisorError::Forbbiden);
        }

        let mut listeners = &mut self.listeners;

        match listeners.get_mut(&app_id) {
            Some(vector) => {
                vector.push(listener);
            }
            None => {
                listeners.insert(app_id.into(), vec![listener]);
            }
        };

        Ok(())
    }

    fn register_runner(
        &mut self,
        app_id: impl Into<String>,
        runner: Box<dyn VRunnable>,
    ) -> Result<(), VSupervisorError> {
        self.runners.insert(app_id.into(), runner);
        Ok(())
    }

    async fn exec(&self, cmd: CommandEvelope) {}
}

#[cfg(test)]
mod supervisor_test {
    use async_trait::async_trait;
    use std::collections::HashMap;

    use crate::{
        AppInfo, AppMetadata, AppPermission, SerializedEvent, VAppsState, VListener, VQuery,
        VSupervisor,
    };

    use super::SingleThreadSuperVisor;

    fn get_state() -> VAppsState {
        let mut apps: VAppsState = HashMap::new();

        apps.insert(
            "com.app.account".into(),
            AppMetadata {
                app_info: AppInfo {
                    author: "hello".into(),
                    description: "hello".into(),
                    id: "com.app.account".into(),
                    name: "Account".into(),
                    permissions: vec![AppPermission {
                        name: "Wallet".into(),
                        description: "an amazing desc".into(),
                        app: "com.app.wallet".into(),
                        cmds: vec!["Sign".into()],
                        events: vec!["Signed".into()],
                    }],
                    version: "0.0.1".into(),
                },
                channel_id: "some-channel-id".into(),
            },
        );

        apps.insert(
            "com.app.wallet".into(),
            AppMetadata {
                app_info: AppInfo {
                    author: "hello".into(),
                    description: "hello".into(),
                    id: "com.app.wallet".into(),
                    name: "Walllet".into(),
                    permissions: vec![AppPermission {
                        name: "Blockchain".into(),
                        description: "an amazing desc".into(),
                        app: "com.app.blockchain".into(),
                        cmds: vec!["Send".into()],
                        events: vec!["Published".into()],
                    }],
                    version: "0.0.1".into(),
                },
                channel_id: "some-channel-id".into(),
            },
        );

        apps
    }

    #[derive(Default)]
    struct MockLogger {}

    #[async_trait]
    impl VQuery for MockLogger {
        async fn dispatch(&self, _: &str, _: &[SerializedEvent]) {}
    }

    pub fn create_mock_listener(
        from_app_id: impl Into<String>,
        event: impl Into<String>,
    ) -> VListener {
        VListener {
            from_app_id: from_app_id.into(),
            event: event.into(),
            handler: Box::new(MockLogger::default()),
        }
    }

    #[test]
    fn check_permission_event() {
        let state = get_state();
        let supervisor = SingleThreadSuperVisor::new(state);

        let app = "com.app.account";
        let listener = create_mock_listener("com.app.wallet", "Signed");

        let is_allowed =
            supervisor.check_permission_to_listen(app, &listener.from_app_id, &listener.event);
        assert_eq!(is_allowed, true);

        let listener = create_mock_listener("com.app.wallet", "Published");

        let is_allowed =
            supervisor.check_permission_to_listen(app, &listener.from_app_id, &listener.event);
        assert_eq!(is_allowed, false);
    }
}
