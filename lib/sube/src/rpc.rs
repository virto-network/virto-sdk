use core::fmt::Write;
use serde::{Deserialize, Serialize};

use crate::meta::{self, Metadata};
use crate::prelude::*;
use crate::Backend;
use crate::Error;
use meta::from_bytes;

/// Hex-encode bytes with `0x` prefix into an existing String, avoiding a new allocation.
fn push_hex(buf: &mut String, bytes: &[u8]) {
    buf.push_str("0x");
    for &b in bytes {
        let _ = write!(buf, "{:02x}", b);
    }
}

/// Hex-encode bytes with `0x` prefix, returning a new String.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    push_hex(&mut s, bytes);
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
    pub id: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn into_result(self) -> Result<serde_json::Value, JsonRpcError> {
        if let Some(err) = self.error {
            return Err(err);
        }
        self.result
            .ok_or_else(|| JsonRpcError::new(-1, "no result"))
    }
}

/// A JSON-RPC notification (subscription event) — has `method` and `params` but no `id`.
#[derive(Deserialize, Debug)]
pub struct Notification {
    pub method: String,
    pub params: NotificationParams,
}

#[derive(Deserialize, Debug)]
pub struct NotificationParams {
    pub subscription: String,
    pub result: serde_json::Value,
}

/// Represents either a response (has `id`) or a notification (has `method` + `params.subscription`).
#[derive(Debug)]
pub enum IncomingMessage {
    Response(JsonRpcResponse),
    Notification(Notification),
}

impl IncomingMessage {
    /// Parse a JSON string into either a Response or Notification.
    pub fn parse(json: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Raw {
            id: Option<serde_json::Value>,
            result: Option<serde_json::Value>,
            error: Option<JsonRpcError>,
            method: Option<String>,
            params: Option<serde_json::Value>,
        }

        let raw: Raw = serde_json::from_str(json).ok()?;

        if raw.id.as_ref().is_some_and(|v| !v.is_null()) || raw.method.is_none() {
            Some(IncomingMessage::Response(JsonRpcResponse {
                id: raw.id,
                result: raw.result,
                error: raw.error,
            }))
        } else {
            let params: NotificationParams = serde_json::from_value(raw.params?).ok()?;
            Some(IncomingMessage::Notification(Notification {
                method: raw.method?,
                params,
            }))
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_hex_encodes_bytes() {
        assert_eq!(to_hex(&[0xde, 0xad]), "0xdead");
    }

    #[test]
    fn to_hex_empty() {
        assert_eq!(to_hex(&[]), "0x");
    }

    #[test]
    fn parse_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":"0x1234"}"#;
        let msg = IncomingMessage::parse(json).unwrap();
        match msg {
            IncomingMessage::Response(resp) => {
                assert_eq!(resp.id, Some(serde_json::json!(1)));
                assert_eq!(resp.result, Some(serde_json::json!("0x1234")));
            }
            _ => panic!("expected Response"),
        }
    }

    #[test]
    fn parse_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"chainHead_v1_followEvent","params":{"subscription":"sub1","result":{"event":"initialized","finalizedBlockHashes":["0xabc"]}}}"#;
        let msg = IncomingMessage::parse(json).unwrap();
        match msg {
            IncomingMessage::Notification(notif) => {
                assert_eq!(notif.method, "chainHead_v1_followEvent");
                assert_eq!(notif.params.subscription, "sub1");
            }
            _ => panic!("expected Notification"),
        }
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(IncomingMessage::parse("not json at all").is_none());
    }

    #[test]
    fn response_into_result_ok() {
        let resp = JsonRpcResponse {
            id: Some(serde_json::json!(1)),
            result: Some(serde_json::json!("ok")),
            error: None,
        };
        let val = resp.into_result().unwrap();
        assert_eq!(val, serde_json::json!("ok"));
    }

    #[test]
    fn response_into_result_error() {
        let resp = JsonRpcResponse {
            id: Some(serde_json::json!(1)),
            result: None,
            error: Some(JsonRpcError::new(-1, "fail")),
        };
        let err = resp.into_result().unwrap_err();
        assert_eq!(err.code, -1);
        assert_eq!(err.message, "fail");
    }
}

// --- Subscription type ---

/// A subscription stream that yields JSON values from the node.
#[cfg(any(feature = "ws", feature = "smoldot"))]
pub struct Subscription {
    pub(crate) rx: futures_channel::mpsc::UnboundedReceiver<serde_json::Value>,
}

#[cfg(any(feature = "ws", feature = "smoldot"))]
impl Subscription {
    pub async fn next(&mut self) -> Option<serde_json::Value> {
        use futures_util::StreamExt;
        self.rx.next().await
    }
}

// --- Rpc trait ---

/// Rpc defines types of backends that are remote and talk JSON-RPC
#[allow(async_fn_in_trait)]
pub trait Rpc {
    async fn rpc(&self, method: &str, params: serde_json::Value) -> RpcResult<serde_json::Value>;
}

/// Backends that support JSON-RPC subscriptions (WebSocket, smoldot).
#[cfg(any(feature = "ws", feature = "smoldot"))]
#[allow(async_fn_in_trait)]
pub trait RpcSubscription: Rpc {
    /// Subscribe to a method. Returns (subscription_id, receiver).
    async fn subscribe(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> RpcResult<(String, Subscription)>;

    /// Unsubscribe from a subscription.
    async fn unsubscribe(&self, method: &str, sub_id: &str) -> RpcResult<()>;
}

// --- Generic HTTP transport (no_std compatible) ---

use core::future::Future;

/// A generic JSON-RPC backend over HTTP.
///
/// Works in any environment — std, no_std, embassy, WASM — by taking
/// an async function that performs the HTTP POST.
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
    async fn rpc(&self, method: &str, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params: Some(params),
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

// --- RpcClient: legacy Backend impl for HTTP-only transports ---

pub struct RpcClient<R>(pub R);

impl<R: Rpc> Backend for RpcClient<R> {
    async fn get_storage_items(
        &self,
        keys: Vec<crate::RawKey>,
        block: Option<u32>,
    ) -> crate::Result<Vec<(Vec<u8>, Option<Vec<u8>>)>> {
        let hex_keys: Vec<String> = keys.iter().map(|v| to_hex(v)).collect();

        let params = if let Some(block_number) = block {
            let info = self
                .block_info(Some(block_number))
                .await
                .map_err(|_| Error::BadBlockNumber)?;
            serde_json::json!([hex_keys, to_hex(&info.hash)])
        } else {
            serde_json::json!([hex_keys])
        };

        let result: Vec<crate::StorageChangeSet> =
            serde_json::from_value(self.0.rpc("state_queryStorageAt", params).await.map_err(
                |err| {
                    log::error!("error state_queryStorageAt {:?}", err);
                    crate::Error::StorageKeyNotFound
                },
            )?)
            .map_err(|e| crate::Error::Decode(e.to_string()))?;

        let result: Vec<_> = match result.into_iter().next() {
            None => vec![],
            Some(change_set) => change_set
                .changes
                .into_iter()
                .map(|(k, v)| {
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
        from: crate::RawKey,
        size: u16,
        to: Option<crate::RawKey>,
    ) -> crate::Result<Vec<crate::RawKey>> {
        let start_key = to_hex(&to.unwrap_or_else(|| from.clone()));
        let params = serde_json::json!([to_hex(&from), size, start_key]);

        let result: Vec<String> =
            serde_json::from_value(self.0.rpc("state_getKeysPaged", params).await.map_err(
                |err| {
                    log::error!("error paged {:?}", err);
                    crate::Error::StorageKeyNotFound
                },
            )?)
            .map_err(|e| crate::Error::Decode(e.to_string()))?;

        let keys = result
            .into_iter()
            .map(|k| {
                hex::decode(&k[2..]).map_err(|_| crate::Error::Decode("hex decode failed".into()))
            })
            .collect::<crate::Result<Vec<_>>>()?;
        Ok(keys)
    }

    async fn submit(&self, ext: &[u8]) -> crate::Result<()> {
        let extrinsic = to_hex(ext);
        log::debug!("Extrinsic: {}", extrinsic);

        self.0
            .rpc("author_submitExtrinsic", serde_json::json!([extrinsic]))
            .await
            .map_err(|e| crate::Error::Node(e.to_string()))?;

        Ok(())
    }

    async fn metadata(&self) -> crate::Result<Metadata> {
        let res: String = serde_json::from_value(
            self.0
                .rpc("state_getMetadata", serde_json::json!([]))
                .await
                .map_err(|e| crate::Error::Node(e.to_string()))?,
        )
        .map_err(|e| crate::Error::Decode(e.to_string()))?;
        let response = hex::decode(&res[2..])
            .map_err(|_err| crate::Error::Decode("metadata hex decode failed".into()))?;
        let meta = from_bytes(&mut response.as_slice()).map_err(|_| crate::Error::BadMetadata)?;
        log::trace!("Metadata {:#?}", meta);
        Ok(meta)
    }

    async fn block_info(&self, at: Option<u32>) -> crate::Result<meta::BlockInfo> {
        let params = match at {
            Some(n) => serde_json::json!([n]),
            None => serde_json::json!([]),
        };

        let hex_str: String = serde_json::from_value(
            self.0
                .rpc("chain_getBlockHash", params)
                .await
                .map_err(|e| crate::Error::Node(e.to_string()))?,
        )
        .map_err(|e| crate::Error::Decode(e.to_string()))?;

        let mut hash = [0u8; 32];
        hex::decode_to_slice(&hex_str[2..], &mut hash)
            .map_err(|_| crate::Error::Decode("hex decode failed".into()))?;

        Ok(meta::BlockInfo {
            number: at.unwrap_or(0) as u64,
            hash,
            parent: hash,
        })
    }
}
