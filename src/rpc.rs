use async_trait::async_trait;
use frame_metadata::RuntimeMetadataPrefixed;
use futures_lite::AsyncReadExt;
use jsonrpc::serde_json::value::RawValue;
pub use jsonrpc::{error, Error, Request, Response};

use crate::{meta_ext::MetaExt, Backend, StorageKey};

pub type RpcResult = Result<Vec<u8>, error::Error>;

/// Rpc defines types of backends that are remote and talk JSONRpc
#[async_trait]
pub trait Rpc: Backend + Send + Sync {
    async fn rpc(&self, method: &str, params: &[&str]) -> RpcResult;

    fn convert_params(params: &[&str]) -> Vec<Box<RawValue>> {
        params
            .iter()
            .map(|p| format!("\"{}\"", p))
            .map(RawValue::from_string)
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    }
}

#[async_trait]
impl<R: Rpc> Backend for R {
    async fn query_bytes<K>(&self, key: K) -> crate::Result<Vec<u8>>
    where
        K: std::convert::TryInto<StorageKey, Error = crate::Error> + Send,
    {
        let key = key.try_into()?.to_string();
        log::debug!("StorageKey encoded: {}", key);
        self.rpc("state_getStorage", &[&key])
            .await
            // NOTE it could fail for more reasons
            .map_err(|_| crate::Error::StorageKeyNotFound)
    }

    async fn submit<T>(&self, mut ext: T) -> crate::Result<()>
    where
        T: futures_lite::AsyncRead + Send + Unpin,
    {
        let mut extrinsic = vec![];
        ext.read_to_end(&mut extrinsic)
            .await
            .map_err(|_| crate::Error::BadInput)?;
        let extrinsic = format!("0x{}", hex::encode(&extrinsic));
        log::debug!("Extrinsic: {}", extrinsic);

        let res = self
            .rpc("author_submitExtrinsic", &[&extrinsic])
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;
        log::debug!("Extrinsic {:x?}", res);
        Ok(())
    }

    async fn metadata(&self) -> crate::Result<frame_metadata::RuntimeMetadataPrefixed> {
        let meta = self
            .rpc("state_getMetadata", &[])
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        let meta = RuntimeMetadataPrefixed::from_bytes(meta).map_err(|_| crate::Error::BadMetadata);
        log::trace!("Metadata {:#?}", meta);
        meta
    }
}

