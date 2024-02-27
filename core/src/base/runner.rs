use crate::utils;
use async_trait::async_trait;
use serde_json::Value;

pub enum VRunnerError {
    Unknown,
}

#[async_trait]
pub trait VRunner {
    async fn exec<'a>(
        &self,
        aggregaate_id: &'a str,
        command: Value,
        metadata: utils::HashMap<String, String>,
    ) -> Result<(), VRunnerError>;
}
