use super::app::AppInfo;
use async_trait::async_trait;

#[derive(Debug)]
pub enum AppRegistryError {
    AlreadyInstalled,
    Unknown,
    CantAddApp(String),
    CantUninstall(String),
}

pub type AppRegistryResult<T> = Result<T, AppRegistryError>;

#[async_trait]
pub trait VRegistry {
    async fn is_registered(&self, app_info: &AppInfo) -> AppRegistryResult<bool>;
    async fn add(&self, app_info: &AppInfo) -> AppRegistryResult<()>;
    async fn remove(&self, id: &AppInfo) -> AppRegistryResult<()>;
    async fn list_apps(&self) -> AppRegistryResult<Vec<AppInfo>>;
}

pub trait VAppInfo {
    fn get_app_info(&self) -> &AppInfo;
}
