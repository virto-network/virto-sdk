use core::fmt::Write;
use serde::{Deserialize, Serialize};

use crate::meta::{self, Metadata};
use crate::Backend;
use crate::Error;
use crate::{prelude::*, RawKey as RawStorageKey, StorageChangeSet};
use meta::from_bytes;

/// Hex-encode bytes with `0x` prefix into an existing String, avoiding a new allocation.
fn push_hex(buf: &mut String, bytes: &[u8]) {
    buf.push_str("0x");
    for &b in bytes {
        let _ = write!(buf, "{:02x}", b);
    }
}

/// Hex-encode bytes with `0x` prefix, returning a new String.
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    push_hex(&mut s, bytes);
    s
}

/// Hex-encode bytes as a quoted JSON string: `"0x..."`.
fn to_quoted_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(4 + bytes.len() * 2);
    s.push('"');
    push_hex(&mut s, bytes);
    s.push('"');
    s
}

// --- Inline JSON-RPC protocol types ---

#[derive(Serialize)]
pub struct JsonRpcRequest<'a> {
    pub jsonrpc: &'a str,
    pub id: u32,
    pub method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct JsonRpcResponse {
    pub id: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn into_result<T: for<'de> Deserialize<'de>>(self) -> Result<T, JsonRpcError> {
        if let Some(err) = self.error {
            return Err(err);
        }
        let val = self
            .result
            .ok_or_else(|| JsonRpcError::new(-1, "no result"))?;
        serde_json::from_value(val)
            .map_err(|e| JsonRpcError::new(-32700, &format!("parse error: {e}")))
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcError {
    pub fn new(code: i64, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl core::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC error {}: {}", self.code, self.message)
    }
}

pub type RpcResult<T> = Result<T, JsonRpcError>;

// --- Rpc trait ---

/// Rpc defines types of backends that are remote and talk JSON-RPC
#[allow(async_fn_in_trait)]
pub trait Rpc {
    async fn rpc<T>(&self, method: &str, params: &[&str]) -> RpcResult<T>
    where
        T: for<'de> Deserialize<'de>;

    fn build_params(params: &[&str]) -> serde_json::Value {
        let array = format!("[{}]", params.join(","));
        serde_json::from_str(&array).expect("valid JSON params")
    }
}

// --- RpcClient ---

pub struct RpcClient<R>(pub R);

impl<R: Rpc> Backend for RpcClient<R> {
    async fn get_storage_items(
        &self,
        keys: Vec<RawStorageKey>,
        block: Option<u32>,
    ) -> crate::Result<Vec<(Vec<u8>, Option<Vec<u8>>)>> {
        let keys = serde_json::to_string(&keys.iter().map(|v| to_hex(v)).collect::<Vec<_>>())
            .expect("it to be a valid json");

        let params: Vec<String> = if let Some(block_number) = block {
            let info = self
                .block_info(Some(block_number))
                .await
                .map_err(|_| Error::BadBlockNumber)?;

            vec![keys, to_quoted_hex(&info.hash)]
        } else {
            vec![keys]
        };

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

        let result: Vec<_> = match result.into_iter().next() {
            None => vec![],
            Some(change_set) => change_set
                .changes
                .into_iter()
                .map(|(k, v)| {
                    log::debug!("key: {:?} value: {:?}", k, v);

                    let key = hex::decode(&k[2..])
                        .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;
                    let value = v
                        .map(|v| hex::decode(&v[2..]))
                        .transpose()
                        .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;
                    Ok((key, value))
                })
                .collect::<crate::Result<Vec<_>>>()?,
        };

        Ok(result)
    }

    async fn get_keys_paged(
        &self,
        from: RawStorageKey,
        size: u16,
        to: Option<RawStorageKey>,
    ) -> crate::Result<Vec<RawStorageKey>> {
        let result: Vec<String> = self
            .0
            .rpc(
                "state_getKeysPaged",
                &[
                    &to_quoted_hex(&from),
                    &size.to_string(),
                    &to.or(Some(from)).map(|f| to_quoted_hex(&f)).unwrap(),
                ],
            )
            .await
            .map_err(|err| {
                log::error!("error paged {:?}", err);
                crate::Error::StorageKeyNotFound
            })?;
        log::info!("rpc call {:?}", result);
        let keys = result
            .into_iter()
            .map(|k| {
                hex::decode(&k[2..]).map_err(|_| crate::Error::Decode("hex decode failed".into()))
            })
            .collect::<crate::Result<Vec<_>>>()?;
        Ok(keys)
    }

    async fn submit(&self, ext: &[u8]) -> crate::Result<()> {
        let extrinsic = to_quoted_hex(ext);
        log::debug!("Extrinsic: {}", extrinsic);

        self.0
            .rpc::<serde_json::Value>("author_submitExtrinsic", &[&extrinsic])
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
        let response = hex::decode(&res[2..])
            .map_err(|_err| crate::Error::Decode("metadata hex decode failed".into()))?;
        let meta = from_bytes(&mut response.as_slice()).map_err(|_| crate::Error::BadMetadata)?;
        log::trace!("Metadata {:#?}", meta);
        Ok(meta)
    }

    async fn block_info(&self, at: Option<u32>) -> crate::Result<meta::BlockInfo> {
        #[inline]
        async fn block_hash(s: &impl Rpc, params: &[&str]) -> crate::Result<[u8; 32]> {
            let hex_str: String = s
                .rpc("chain_getBlockHash", params)
                .await
                .map_err(|e| crate::Error::Node(e.to_string()))?;

            let mut hash = [0u8; 32];
            hex::decode_to_slice(&hex_str.as_str()[2..], &mut hash)
                .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;
            Ok(hash)
        }

        let hash = if let Some(block_number) = at {
            let block_number = block_number.to_string();
            block_hash(&self.0, &[&block_number]).await?
        } else {
            block_hash(&self.0, &[]).await?
        };

        Ok(meta::BlockInfo {
            number: at.unwrap_or(0) as u64,
            hash,
            parent: hash,
        })
    }
}

// --- Generic HTTP transport (no_std compatible) ---

use core::future::Future;

/// A generic JSON-RPC backend over HTTP.
///
/// Works in any environment — std, no_std, embassy, WASM — by taking
/// an async function that performs the HTTP POST.
///
/// # Example (with reqwless on embassy)
///
/// ```rust,ignore
/// use sube::rpc::{HttpTransport, RpcClient};
///
/// let transport = HttpTransport::new("http://10.0.0.1:9933", |url, body| async move {
///     // Use your HTTP client here (reqwless, embassy-net, etc.)
///     let response_bytes = my_http_post(url, body).await?;
///     Ok(response_bytes)
/// });
/// let backend = RpcClient(transport);
/// let meta = backend.metadata().await?;
/// let response = sube::query(&backend, &meta, "system/account/0x1234", None).await?;
/// ```
pub struct HttpTransport<F> {
    url: String,
    post: F,
}

impl<F> HttpTransport<F> {
    pub fn new(url: &str, post: F) -> Self {
        HttpTransport {
            url: url.into(),
            post,
        }
    }
}

impl<F, Fut> Rpc for HttpTransport<F>
where
    F: Fn(&str, Vec<u8>) -> Fut,
    Fut: Future<Output = core::result::Result<Vec<u8>, crate::Error>>,
{
    async fn rpc<T>(&self, method: &str, params: &[&str]) -> RpcResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params: Some(Self::build_params(params)),
        };

        let body =
            serde_json::to_vec(&request).map_err(|e| JsonRpcError::new(-32700, &e.to_string()))?;

        let response_bytes = (self.post)(&self.url, body)
            .await
            .map_err(|e| JsonRpcError::new(-32000, &e.to_string()))?;

        let response: JsonRpcResponse = serde_json::from_slice(&response_bytes)
            .map_err(|e| JsonRpcError::new(-32700, &e.to_string()))?;

        response.into_result()
    }
}
