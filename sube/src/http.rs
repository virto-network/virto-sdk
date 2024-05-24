use crate::prelude::*;
use async_trait::async_trait;
use serde::Deserialize;
use core::{convert::TryInto, fmt};
use jsonrpc::{
    error::{standard_error, StandardError},
    serde_json::{to_string, value::to_raw_value},
};
pub use surf::Url;

use crate::rpc::{self, Rpc, RpcResult};

#[derive(Debug)]
pub struct Backend(Url);

impl Backend {
    pub fn new<U>(url: U) -> Self
    where
        U: TryInto<Url>,
        <U as TryInto<Url>>::Error: fmt::Debug,
    {
        Backend(url.try_into().expect("Url"))
    }
}

#[async_trait]
impl Rpc for Backend {
    /// HTTP based JSONRpc request expecting an hex encoded result
    async fn rpc<T: for<'a> Deserialize<'a>>(&self, method: &str, params: &[&str]) -> RpcResult<T> {
        log::info!("RPC `{}` to {}", method, &self.0);
        let req = surf::post(&self.0).content_type("application/json").body(
            to_string(&rpc::Request {
                id: 1.into(),
                jsonrpc: Some("2.0"),
                method,
                params: &Self::convert_params(params),
            })
            .unwrap(),
        );
        let client = surf::client().with(surf::middleware::Redirect::new(2));
        let mut res = client
            .send(req)
            .await
            .map_err(|err| rpc::Error::Transport(err.into_inner().into()))?;

        let status = res.status();
        let res = if status.is_success() {
            res.body_json::<rpc::Response>()
                .await
                .map_err(|err| {
                    standard_error(
                        StandardError::ParseError,
                        Some(to_raw_value(&err.to_string()).unwrap()),
                    )
                })?
                .result::<T>()?
        } else {
            log::debug!("RPC HTTP status: {}", res.status());
            let err = res
                .body_string()
                .await
                .unwrap_or_else(|_| status.canonical_reason().into());
            let err = to_raw_value(&err).expect("error string");

            return Err(if status.is_client_error() {
                standard_error(StandardError::InvalidRequest, Some(err)).into()
            } else {
                standard_error(StandardError::InternalError, Some(err)).into()
            });
        };

        Ok(res)
    }
}
