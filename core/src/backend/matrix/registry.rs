use crate::base::{
    AppInfo, AppMetadata, AppRegistryError, Registry, RegistryResult, RegistryState,
};

use super::prelude::*;
use crate::utils::prelude::*;

#[derive(Debug, Clone)]
pub struct MatrixRegistry {
    client: Client,
}

impl MatrixRegistry {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    async fn get_state(&self) -> Result<MatrixAppsStateContent, AppRegistryError> {
        let account_data_vapp = self
            .client
            .account()
            .account_data::<MatrixAppsStateContent>()
            .await
            .map_err(|_| AppRegistryError::Unknown)?;

        let raw_vapp = account_data_vapp.unwrap_or(
            Raw::new(&MatrixAppsStateContent::default()).map_err(|_| AppRegistryError::Unknown)?,
        );

        Ok(raw_vapp
            .deserialize()
            .map_err(|_| AppRegistryError::Unknown)?)
    }

    async fn add_app(
        &self,
        app_info: &AppInfo,
        room: Room,
    ) -> Result<RegistryState, AppRegistryError> {
        let mut vapps = self.get_state().await?;

        vapps.apps.insert(
            app_info.id.clone().into(),
            AppMetadata {
                app_info: app_info.clone(),
                channel_id: room.room_id().to_string(),
            },
        );

        self.client
            .account()
            .set_account_data(vapps.clone())
            .await
            .map_err(|_| AppRegistryError::Unknown)?;

        Ok(vapps.apps.clone())
    }

    async fn remove_app(&self, app_info: &AppInfo) -> Result<RegistryState, AppRegistryError> {
        let mut vapps = self.get_state().await?;
        vapps.apps.remove(&app_info.id);

        self.client
            .account()
            .set_account_data(vapps.clone())
            .await
            .map_err(|_| AppRegistryError::Unknown)?;

        Ok(vapps.apps.clone())
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

impl Registry for MatrixRegistry {
    async fn add(&self, info: &AppInfo) -> RegistryResult<RegistryState> {
        if self.is_registered(info).await? {
            return Err(AppRegistryError::AlreadyInstalled);
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
            .map_err(|e| AppRegistryError::CantAddApp(e.to_string()))?;

        self.add_app(info, room).await
    }

    async fn remove(&self, info: &AppInfo) -> RegistryResult<RegistryState> {
        let room = self.get_room(info).ok_or(AppRegistryError::CantUninstall(
            "Can't get installed room".to_string(),
        ))?;

        room.leave()
            .await
            .map_err(|_| AppRegistryError::CantUninstall("Can't leave the room".to_string()))?;

        room.forget()
            .await
            .map_err(|_| AppRegistryError::CantUninstall("Can't forget the room".to_string()))?;

        self.remove_app(info).await
    }

    async fn is_registered(&self, info: &AppInfo) -> RegistryResult<bool> {
        let state = self.get_state().await?;
        Ok(state.apps.get(&info.id).is_some())
    }

    async fn list_apps(&self) -> RegistryResult<Vec<AppInfo>> {
        let state = self.get_state().await?;
        Ok(state
            .apps
            .values()
            .into_iter()
            .map(|x| x.app_info.to_owned())
            .collect())
    }
}

#[cfg(test)]
mod manager_test {
    use ::std::error::Error;

    use super::MatrixRegistry;
    use crate::{base::AppInfo, base::Registry, SDKBuilder, SDKCore};
    use async_once_cell::OnceCell;
    use ctor::ctor;
    use tokio::test;
    use tokio::time::{sleep, Duration};
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
                                .with_credentials("fooaccount2", "H0l4mund0@123")
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
    async fn app_registry_lifecycle() {
        let sdkCore = get_sdk().await;

        let app_info = AppInfo {
            description: "foo".into(),
            name: "wallet".into(),
            id: "com.virto.wallet".into(),
            author: "hello@virto.net".into(),
            version: "0.0.1".into(),
            permissions: vec![],
        };

        sdkCore.next_sync().await;

        let manager = MatrixRegistry::new(sdkCore.client());

        manager.remove_app(&app_info).await;
        let state_d = manager.get_state().await.expect("hello world");

        assert_eq!(
            manager
                .is_registered(&app_info)
                .await
                .expect("error checking installed app"),
            false
        );

        assert!(manager.add(&app_info).await.is_ok());

        sdkCore.next_sync().await;

        let state = manager.get_state().await.expect("hello");

        assert_eq!(state.apps.get(&app_info.id).unwrap().app_info, app_info);

        let state = manager.list_apps().await.expect("It must list the apps");

        assert_eq!(state.len(), 1);

        sdkCore.next_sync().await;

        assert!(manager.remove(&app_info).await.is_ok());

        sdkCore.next_sync().await;

        let state = manager.get_state().await.expect("cant get reload event");

        assert!(state.apps.get(&app_info.id).is_none());
    }
}
