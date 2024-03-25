// use std::fmt::{Debug, Write};

// use crate::utils::prelude::*;

// pub enum VRunnerError {
//     Unknown,
// }

// #[derive(Serialize, Clone, Debug, Deserialize)]
// pub struct AppMetadata {
//     pub app_info: AppInfo,
//     pub channel_id: String,
// }

// #[derive(Debug)]
// pub enum VSupervisorError {
//     Unknown,
//     Forbidden,
//     NotRegistered,
// }

// #[async_trait]
// pub trait VSupervisor {
//     async fn run(
//         &mut self,
//         state: &VAppsState,
//         cmd: &CommandEvelope,
//     ) -> Result<(), VSupervisorError>;
// }

// impl SimpleSuperVisor {
//     pub fn new() -> Self {
//         Self {
//             runners: HashMap::new(),
//             listeners: HashMap::new(),
//         }
//     }

//     fn check_permission_to_listen(
//         state: &VAppsState,
//         app_id: &str,
//         from_app_id: &str,
//         event: &str,
//     ) -> bool {
//         if let Some(metadata) = state.get(app_id) {
//             if let Some(permission) = metadata
//                 .app_info
//                 .permissions
//                 .iter()
//                 .find(|p| p.app == from_app_id)
//             {
//                 return permission.events.iter().any(|p| p == event);
//             }
//         }
//         false
//     }

//     fn check_permission_cmd(
//         state: &VAppsState,
//         app_id: &str,
//         from_app_id: &str,
//         cmd: &str,
//     ) -> bool {
//         if let Some(metadata) = state.get(app_id) {
//             if let Some(permission) = metadata
//                 .app_info
//                 .permissions
//                 .iter()
//                 .find(|p| p.app == from_app_id)
//             {
//                 return permission.cmds.iter().any(|p| p == cmd);
//             }
//         }
//         false
//     }
// }

// #[cfg(test)]
// mod supervisor_test {
//     use async_trait::async_trait;
//     use std::collections::HashMap;

//     use crate::{
//         utils, AppInfo, AppMetadata, AppPermission, AppRunnable, SerializedEvent, VAppsState,
//         VListener, VQuery, VRunnerError, VSupervisor,
//     };

//     use super::SimpleSuperVisor;
//     use serde_json::Value;

//     fn get_state() -> VAppsState {
//         let mut apps: VAppsState = HashMap::new();

//         apps.insert(
//             "com.app.account".into(),
//             AppMetadata {
//                 app_info: AppInfo {
//                     author: "hello".into(),
//                     description: "hello".into(),
//                     id: "com.app.account".into(),
//                     name: "Account".into(),
//                     permissions: vec![AppPermission {
//                         name: "Wallet".into(),
//                         description: "an amazing desc".into(),
//                         app: "com.app.wallet".into(),
//                         cmds: vec!["Sign".into()],
//                         events: vec!["Signed".into()],
//                     }],
//                     version: "0.0.1".into(),
//                 },
//                 channel_id: "some-channel-id".into(),
//             },
//         );

//         apps.insert(
//             "com.app.wallet".into(),
//             AppMetadata {
//                 app_info: AppInfo {
//                     author: "hello".into(),
//                     description: "hello".into(),
//                     id: "com.app.wallet".into(),
//                     name: "Walllet".into(),
//                     permissions: vec![AppPermission {
//                         name: "Blockchain".into(),
//                         description: "an amazing desc".into(),
//                         app: "com.app.blockchain".into(),
//                         cmds: vec!["Send".into()],
//                         events: vec!["Published".into()],
//                     }],
//                     version: "0.0.1".into(),
//                 },
//                 channel_id: "some-channel-id".into(),
//             },
//         );

//         apps
//     }

//     #[derive(Default)]
//     struct MockLogger {}

//     #[async_trait]
//     impl VQuery for MockLogger {
//         async fn dispatch(&self, _: &str, _: &[SerializedEvent]) {}
//     }

//     pub fn create_mock_listener(
//         from_app_id: impl Into<String>,
//         event: impl Into<String>,
//     ) -> VListener {
//         VListener {
//             from_app_id: from_app_id.into(),
//             event: event.into(),
//             handler: Box::new(MockLogger::default()),
//         }
//     }

//     #[test]
//     fn check_permission_event() {
//         let state = get_state();
//         let supervisor = SimpleSuperVisor::new();

//         let app = "com.app.account";
//         let listener = create_mock_listener("com.app.wallet", "Signed");

//         let is_allowed = SimpleSuperVisor::check_permission_to_listen(
//             &state,
//             app,
//             &listener.from_app_id,
//             &listener.event,
//         );

//         assert_eq!(is_allowed, true);

//         let listener = create_mock_listener("com.app.wallet", "Published");

//         let is_allowed = SimpleSuperVisor::check_permission_to_listen(
//             &state,
//             app,
//             &listener.from_app_id,
//             &listener.event,
//         );
//         assert_eq!(is_allowed, false);
//     }
// }
