use crate::utils::prelude::*;
use crate::AppInfo;

#[derive(Debug)]
pub enum AppRegistryError {
    AlreadyInstalled,
    Unknown,
    CantAddApp(String),
    CantUninstall(String),
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AppMetadata {
    pub app_info: AppInfo,
    pub channel_id: String,
}

pub type RegistryResult<T> = Result<T, AppRegistryError>;
pub type RegistryState = HashMap<String, AppMetadata>;

pub trait Registry {
    async fn is_registered(&self, id: &str) -> RegistryResult<bool>;
    async fn add(&self, app_info: &AppInfo) -> RegistryResult<RegistryState>;
    async fn remove(&self, id: &AppInfo) -> RegistryResult<RegistryState>;
    async fn list_apps(&self) -> RegistryResult<Vec<AppInfo>>;
}
