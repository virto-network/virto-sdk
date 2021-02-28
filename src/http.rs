use crate::StorageKey;
use async_trait::async_trait;
use jsonrpc::serde_json::{to_string, value::RawValue};
use std::convert::TryInto;
use url::Url;

pub struct Backend(Url);

#[async_trait]
impl crate::Backend for Backend {
    async fn query<K>(&self, key: K) -> crate::Result<String>
    where
        K: TryInto<StorageKey<Self>> + Send,
    {
        let key = key
            .try_into()
            .map_err(|_| crate::Error::BadKey)?
            .to_string();
        self.rpc("state_getStorage", &[&key])
            .await
            .map_err(|_| crate::Error::NotFound)
    }

    async fn submit(_ext: &[u8]) -> crate::Result<()> {
        todo!()
    }
}

impl Backend {
    pub fn new(url: impl Into<Url>) -> Self {
        Backend(url.into())
    }

    /// HTTP based JSONRpc request expecting an hex encoded result
    async fn rpc(
        &self,
        method: &str,
        params: &[&str],
    ) -> Result<String, Box<dyn std::error::Error>> {
        surf::post(&self.0)
            .content_type("application/json")
            .body(
                to_string(&jsonrpc::Request {
                    id: 1.into(),
                    jsonrpc: Some("2.0"),
                    method,
                    params: &params
                        .iter()
                        .map(|p| RawValue::from_string(p.to_string()).unwrap())
                        .collect::<Vec<_>>(),
                })
                .unwrap(),
            )
            .await?
            .body_json::<jsonrpc::Response>()
            .await?
            .result()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}
