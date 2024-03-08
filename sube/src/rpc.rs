use core::convert::TryInto;

use async_trait::async_trait;
use jsonrpc::serde_json::value::RawValue;
pub use jsonrpc::{error, Error, Request, Response};

use crate::meta::{self, Metadata};
use crate::prelude::*;
use crate::{Backend, StorageKey};

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
    async fn query_storage(&self, key: &StorageKey) -> crate::Result<Vec<u8>> {
        let key = key.to_string();
        log::debug!("StorageKey encoded: {}", key);
        self.rpc("state_getStorage", &[&key]).await.map_err(|e| {
            log::debug!("RPC failure: {}", e);
            // NOTE it could fail for more reasons
            crate::Error::StorageKeyNotFound
        })
    }

    async fn submit<T>(&self, ext: T) -> crate::Result<()>
    where
        T: AsRef<[u8]> + Send,
    {
        let extrinsic = format!("0x{}", hex::encode(ext.as_ref()));
        log::debug!("Extrinsic: {}", extrinsic);

        let res = self
            .rpc("author_submitExtrinsic", &[&extrinsic])
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;
        log::debug!("Extrinsic {:x?}", res);
        Ok(())
    }

    async fn metadata(&self) -> crate::Result<Metadata> {
        let meta = self
            .rpc("state_getMetadata", &[])
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        
        let meta = meta::from_bytes(&mut meta.as_slice()).map_err(|_| crate::Error::BadMetadata)?;
        log::trace!("Metadata {:#?}", meta);
        Ok(meta)
    }

    async fn block_info(&self, at: Option<u32>) -> crate::Result<meta::BlockInfo> {
        async fn block_info<R>(s: &R, params: &[&str]) -> crate::Result<Vec<u8>>
        where
            R: Rpc,
        {
            Ok(s.rpc("chain_getBlockHash", params)
                .await
                .map_err(|e| crate::Error::Node(e.to_string()))?)
        }

        let block_hash = if let Some(block_number) = at {
            let block_number = block_number.to_string();
            block_info(self, &[&block_number]).await?
        } else {
            block_info(self, &[]).await?
        };

        // TODO: Make sure to complete this in a future
        // Hint: RPC should not deserialize the JSON-RPC result
        // This produces a deserialization error
        // let block = self
        //     .rpc("chain_getBlock", &[&hex::encode(&block_hash)])
        //     .await
        //     .map_err(|e| crate::Error::Node(e.to_string()))?;

        Ok(meta::BlockInfo {
            number: at.unwrap_or(0) as u64,
            hash: block_hash[0..32]
                .try_into()
                .expect("Block hash is not 32 bytes"),
            parent: block_hash[0..32]
                .try_into()
                .expect("Block hash is not 32 bytes"),
        })
    }
}
