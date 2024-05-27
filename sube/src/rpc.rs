use core::convert::TryInto;

use async_trait::async_trait;
use jsonrpc::error::standard_error;
use jsonrpc::serde_json::value::RawValue;
pub use jsonrpc::{error, Error, Request, Response};
use rand_core::block;
use serde::Deserialize;

use crate::meta::{self, Metadata};
use crate::{prelude::*, StorageChangeSet};
use crate::{Backend, StorageKey};

pub type RpcResult<T> = Result<T, error::Error>;

/// Rpc defines types of backends that are remote and talk JSONRpc
#[async_trait]
pub trait Rpc: Backend + Send + Sync {
    async fn rpc<T: for<'a> Deserialize<'a>>(&self, method: &str, params: &[&str]) -> RpcResult<T>;

    fn convert_params(params: &[&str]) -> Vec<Box<RawValue>> {

        let params_debug = params
            .iter()
            .map(|p| format!("{}", p))
            .map(RawValue::from_string)
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        
        log::info!("rpc_debug {:?}", &params_debug);
        params
            .iter()
            .map(|p| format!("{}", p))
            .map(RawValue::from_string)
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    }
}

#[async_trait]
impl<R: Rpc> Backend for R {
    async fn query_storage_at(
        &self,
        keys: Vec<String>,
        block: Option<String>,
    ) -> crate::Result<Vec<StorageChangeSet>> {
        let keys = serde_json::to_string(&keys).expect("it to be a valid json");
        let params: Vec<String> = if let Some(block_hash) = block {
            vec![keys, block_hash]
        } else {
            vec![keys]
        };

        self.rpc(
            "state_queryStorageAt",
            params
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .await
        .map_err(|err| {
            log::info!("error {:?}", err);
            crate::Error::StorageKeyNotFound
        })
    }

    async fn get_keys_paged(
        &self,
        from: &StorageKey,
        size: u16,
        to: Option<&StorageKey>,
    ) -> crate::Result<Vec<String>> {
        let r: Vec<String> = self
            .rpc(
                "state_getKeysPaged",
                &[
                    &format!("\"{}\"", &from),
                    &size.to_string(),
                    &to.or(Some(&from)).map(|f| format!("\"{}\"", &f)).unwrap(),
                ],
            )
            .await
            .map_err(|err| {
                log::info!("error {:?}", err);
                crate::Error::StorageKeyNotFound
            })?;
        log::info!("rpc call {:?}", r);
        Ok(r)
    }

    async fn query_storage(&self, key: &StorageKey) -> crate::Result<Vec<u8>> {
        let key = key.to_string();
        log::debug!("StorageKey encoded: {}", key);

        let res: String = self
            .rpc("state_getStorage", &[&format!("\"{}\"", &key)])
            .await
            .map_err(|e| {
                log::debug!("RPC failure: {}", e);
                // NOTE it could fail for more reasons
                crate::Error::StorageKeyNotFound
            })?;

        let response = hex::decode(&res[2..]).map_err(|_err| crate::Error::StorageKeyNotFound)?;

        Ok(response)
    }

    async fn submit<T>(&self, ext: T) -> crate::Result<()>
    where
        T: AsRef<[u8]> + Send,
    {
        let extrinsic = format!("0x{}", hex::encode(ext.as_ref()));
        log::debug!("Extrinsic: {}", extrinsic);

        let res = self
            .rpc("author_submitExtrinsic", &[&format!("\"{}\"", &extrinsic)])
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;
        log::debug!("Extrinsic {:x?}", res);
        Ok(())
    }

    async fn metadata(&self) -> crate::Result<Metadata> {
        let res: String = self
            .rpc("state_getMetadata", &[])
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;
        let response = hex::decode(&res[2..]).map_err(|_err| crate::Error::StorageKeyNotFound)?;
        let meta =
            meta::from_bytes(&mut response.as_slice()).map_err(|_| crate::Error::BadMetadata)?;
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