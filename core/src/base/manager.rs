use super::app::AppInfo;
use async_trait::async_trait;

#[derive(Debug)]
pub enum AppManagerError {
    AlreadyInstalled,
    Unknown,
    CantInstall(String),
    CantUninstall(String),
}

pub type AppManagerResult<T> = Result<T, AppManagerError>;

#[async_trait]
pub trait AppManager {
    async fn is_installed(&self, app_info: &AppInfo) -> AppManagerResult<bool>;
    async fn install(&self, app_info: &AppInfo) -> AppManagerResult<()>;
    async fn uninstall(&self, id: &AppInfo) -> AppManagerResult<()>;
    async fn list_apps(&self) -> AppManagerResult<Vec<AppInfo>>;
}
