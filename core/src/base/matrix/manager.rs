use async_trait::async_trait;
use matrix_sdk::{
    ruma::{
        api::client::room::create_room::v3::Request as CreateRoomRequest,
        api::client::room::Visibility,
        events::{room::encryption::RoomEncryptionEventContent, InitialStateEvent},
    },
    Client, Room,
};

use crate::base::{AppInfo, AppManagerError};

use super::super::{AppManager, AppManagerResult};

struct MatrixManager {
    client: Box<Client>,
}

impl MatrixManager {
    fn new(client: Box<Client>) -> Self {
        Self { client }
    }

    fn get_room_id(&self, app_info: &AppInfo) -> String {
        format!("app-{}", app_info.id)
    }

    fn get_room(&self, app_info: &AppInfo) -> Option<Room> {
        self.client
            .joined_rooms()
            .iter()
            .find(|r| r.name().eq(&Some(self.get_room_id(app_info))))
            .map(|r| r.to_owned())
    }
}

#[async_trait]
impl AppManager for MatrixManager {
    async fn install(&self, info: &AppInfo) -> AppManagerResult<()> {
        if self.is_installed(info).await? {
            return Err(AppManagerError::AlreadyInstalled);
        }
        let mut room = CreateRoomRequest::new();

        room.visibility = Visibility::Private;
        room.name = Some(self.get_room_id(info));
        room.initial_state =
            vec![
                InitialStateEvent::new(RoomEncryptionEventContent::with_recommended_defaults())
                    .to_raw_any(),
            ];

        let room = self
            .client
            .create_room(room)
            .await
            .map_err(|_| AppManagerError::CantInstall("Erro Creating the Room".to_string()))?;

        Ok(())
    }

    async fn uninstall(&self, info: &AppInfo) -> AppManagerResult<()> {
        let room = self.get_room(info).ok_or(AppManagerError::CantUninstall(
            "Can't get installed room".to_string(),
        ))?;

        room.leave()
            .await
            .map_err(|_| AppManagerError::CantUninstall("Can't leave the room".to_string()))?;

        room.forget()
            .await
            .map_err(|_| AppManagerError::CantUninstall("Can't forget the room".to_string()))?;
        Ok(())
    }

    async fn is_installed(&self, info: &AppInfo) -> AppManagerResult<bool> {
        Ok(self.get_room(info).is_some())
    }

    async fn list_apps(&self) -> AppManagerResult<Vec<AppInfo>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod manager_test {
    use ::std::error::Error;

    use super::MatrixManager;
    use crate::{base::AppInfo, base::AppManager, SDKBuilder, SDKCore};
    use async_once_cell::OnceCell;
    use ctor::ctor;
    use tokio::time::{sleep, Duration};
    use tokio::test;
    use tracing_subscriber::fmt::init as InitLogger;

    static mut SDK_CORE: OnceCell<SDKCore> = OnceCell::new();

    async fn get_sdk() -> &'static mut SDKCore {
        unsafe {
            match SDK_CORE.get() {
                Some(..) => SDK_CORE.get_mut().expect("error getting mut"),
                None => {
                    SDK_CORE
                        .get_or_try_init(async {
                            let mut core = SDKBuilder::new()
                                .with_homeserver("https://matrix-client.matrix.org")
                                .with_credentials("myfooaccoount", "H0l4mund0@123")
                                .with_device_name("iphone-dev")
                                .build_and_login()
                                .await
                                .expect("Can not login");

                            core.init().await;

                            Ok::<_, ()>(core)
                        })
                        .await
                        .expect("Erroor at getting core");

                    SDK_CORE.get_mut().expect("error getting mut")
                }
            }
        }
    }

    #[ctor]
    fn before() {
        // InitLogger();
    }

    #[tokio::test]
    async fn app_0_check() {
        let sdkCore = get_sdk().await;

        let app_info = AppInfo {
            description: "foo".into(),
            name: "wallet".into(),
            id: "com.virto.wallet".into(),
            author: "hello@virto.net".into(),
            version: "0.0.1".into(),
            permission: vec![],
        };

        sdkCore.next_sync().await;

        let manager = MatrixManager::new(sdkCore.client());
        assert_eq!(
            manager
                .is_installed(&app_info)
                .await
                .expect("error checking installed app"),
            false
        )
    }

    #[tokio::test]
    async fn app_1_install() {
        let sdkCore = get_sdk().await;

        let app_info = AppInfo {
            description: "foo".into(),
            name: "wallet".into(),
            id: "com.virto.wallet".into(),
            author: "hello@virto.net".into(),
            version: "0.0.1".into(),
            permission: vec![],
        };

        sdkCore.next_sync().await;

        let manager = MatrixManager::new(sdkCore.client());
        assert_eq!(manager.install(&app_info).await.expect(""), ());
    }

    #[tokio::test]
    async fn app_2_uninstall() {
        let mut sdkCore = get_sdk().await;

        let app_info = AppInfo {
            description: "foo".into(),
            name: "wallet".into(),
            id: "com.virto.wallet".into(),
            author: "hello@virto.net".into(),
            version: "0.0.1".into(),
            permission: vec![],
        };

        sdkCore.next_sync().await;

        let manager = MatrixManager::new(sdkCore.client());
        assert_eq!(
            manager.uninstall(&app_info).await.expect("cant uninstall"),
            ()
        );
    }
}
