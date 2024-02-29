use crate::{utils::HashMap, AppInfo};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct AppMetadata {
    pub app_info: AppInfo,
    pub channel_id: String,
}

pub type VAppsState = HashMap<String, AppMetadata>;
