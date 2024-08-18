use core::convert::TryInto;
use jsonrpc::serde_json::value::RawValue;
pub use jsonrpc::{error, Error, Request, Response};
use serde::Deserialize;

use crate::meta::{self, Metadata};
use crate::{prelude::*, StorageChangeSet};
use crate::{Backend, StorageKey};
use meta::from_bytes;

pub type RpcResult<T> = Result<T, error::Error>;

/// Rpc defines types of backends that are remote and talk JSONRpc
pub trait Rpc {
    async fn rpc<T>(&self, method: &str, params: &[&str]) -> RpcResult<T>
    where
        T: for<'de> Deserialize<'de>;

    fn convert_params(params: &[&str]) -> Vec<Box<RawValue>> {
        params
            .iter()
            .map(|p| p.to_string())
            .map(RawValue::from_string)
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    }
}

pub struct RpcClient<R>(pub R);

impl<R: Rpc> Backend for RpcClient<R> {
    async fn get_storage_items(
        &self,
        keys: Vec<String>,
        block: Option<String>,
    ) -> crate::Result<impl Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        let keys = serde_json::to_string(&keys).expect("it to be a valid json");

        log::info!("query_storage_at encoded: {}", keys);
        let params: Vec<String> = if let Some(block_hash) = block {
            vec![keys, format!("\"{}\"", block_hash)]
        } else {
            vec![keys]
        };

        log::info!("params encoded: {:?}", params);

        let result = self
            .0
            .rpc::<Vec<StorageChangeSet>>(
                "state_queryStorageAt",
                params
                    .iter()
                    .map(|s| s.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .await
            .map_err(|err| {
                log::error!("error state_queryStorageAt {:?}", err);
                crate::Error::StorageKeyNotFound
            })?;

        if let Some(change_set) = result.into_iter().next() {
            let keys_response = change_set.changes.into_iter().map(|[k, v]| {
                log::info!("key: {} value: {}", k, v);

                (
                    hex::decode(&k[2..]).expect("to be an hex"),
                    hex::decode(&v[2..]).expect("to be an hex"),
                )
            });

            Ok(keys_response)
        } else {
            Err(crate::Error::StorageKeyNotFound)
        }
    }

    async fn get_keys_paged(
        &self,
        from: &StorageKey,
        size: u16,
        to: Option<&StorageKey>,
    ) -> crate::Result<Vec<String>> {
        let r: Vec<String> = self
            .0
            .rpc(
                "state_getKeysPaged",
                &[
                    &format!("\"{}\"", &from),
                    &size.to_string(),
                    &to.or(Some(from)).map(|f| format!("\"{}\"", &f)).unwrap(),
                ],
            )
            .await
            .map_err(|err| {
                log::error!("error paged {:?}", err);
                crate::Error::StorageKeyNotFound
            })?;
        log::info!("rpc call {:?}", r);
        Ok(r)
    }

    async fn submit(&self, ext: impl AsRef<[u8]>) -> crate::Result<()> {
        let extrinsic = format!("0x{}", hex::encode(ext.as_ref()));
        log::debug!("Extrinsic: {}", extrinsic);

        self.0
            .rpc::<serde_json::Value>("author_submitExtrinsic", &[&format!("\"{}\"", &extrinsic)])
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        Ok(())
    }

    async fn metadata(&self) -> crate::Result<Metadata> {
        let res: String = self
            .0
            .rpc("state_getMetadata", &[])
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;
        let response =
            hex::decode(&res[2..]).map_err(|_err| crate::Error::CantDecodeReponseForMeta)?;
        let meta = from_bytes(&mut response.as_slice()).map_err(|_| crate::Error::BadMetadata)?;
        log::trace!("Metadata {:#?}", meta);
        Ok(meta)
    }

    async fn block_info(&self, at: Option<u32>) -> crate::Result<meta::BlockInfo> {
        #[inline]
        async fn block_info(s: &impl Rpc, params: &[&str]) -> crate::Result<Vec<u8>> {
            let f = s
                .rpc::<String>("chain_getBlockHash", params)
                .await
                .map_err(|e| crate::Error::Node(e.to_string()));

            Ok(hex::decode(&f?.as_str()[2..]).expect("to be an valid hex"))
        }

        let block_hash = if let Some(block_number) = at {
            let block_number = block_number.to_string();
            block_info(&self.0, &[&block_number]).await?
        } else {
            block_info(&self.0, &[]).await?
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
