use crate::{meta_ext::MetaExt, Error, StorageKey};
use async_trait::async_trait;
use frame_metadata::RuntimeMetadata;
use futures_io::AsyncRead;
use jsonrpc::serde_json::{to_string, value::RawValue};
use std::convert::TryInto;
use url::Url;

#[derive(Debug)]
pub struct Backend(Url);

#[async_trait]
impl crate::Backend for Backend {
    async fn query<K>(&self, key: K) -> crate::Result<String>
    where
        K: TryInto<StorageKey, Error = Error> + Send,
    {
        let key = key.try_into()?.to_string();
        log::debug!("StorageKey encoded: {}", key);
        self.rpc("state_getStorage", &[&key])
            .await
            .map_err(|_| Error::StorageKeyNotFound)
    }

    async fn submit<T>(&self, _ext: T) -> crate::Result<()>
    where
        T: AsyncRead + Send,
    {
        todo!()
    }

    async fn metadata(&self) -> crate::Result<RuntimeMetadata> {
        let meta = self
            .rpc("state_getMetadata", &[])
            .await
            .map_err(|e| Error::Node(e.to_string()))?;
        let meta = hex::decode(&meta[2..]).map_err(|_| Error::BadMetadata)?;
        let meta = RuntimeMetadata::from_bytes(meta).map_err(|_| Error::BadMetadata);
        log::trace!("Metadata {:#?}", meta);
        meta
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
        log::info!("RPC `{}` to {}", method, &self.0);
        surf::post(&self.0)
            .content_type("application/json")
            .body(
                to_string(&jsonrpc::Request {
                    id: 1.into(),
                    jsonrpc: Some("2.0"),
                    method,
                    params: &params
                        .iter()
                        .map(|p| format!("\"{}\"", p))
                        .map(|p| RawValue::from_string(p.to_string()).unwrap())
                        .collect::<Vec<_>>(),
                })
                .unwrap(),
            )
            .await?
            .body_json::<jsonrpc::Response>()
            .await?
            .result()
            .map_err(|e| {
                log::debug!("Rpc error: {}", e);
                Box::new(e) as Box<dyn std::error::Error>
            })
    }
}
