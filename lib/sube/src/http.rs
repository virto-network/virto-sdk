use crate::prelude::*;
use crate::rpc::{self, Rpc, RpcResult};
use core::{convert::TryInto, fmt};
use jsonrpc::{
    error::{standard_error, StandardError},
    serde_json::value::to_raw_value,
};
use reqwest::Client;
use serde::Deserialize;
use url::Url;

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

impl Rpc for Backend {
    /// HTTP based JSONRpc request expecting an hex encoded result
    async fn rpc<T>(&self, method: &str, params: &[&str]) -> RpcResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        log::info!("RPC `{}` to {}", method, &self.0);

        let res = Client::new()
            .post(self.0.to_string())
            .json(&rpc::Request {
                id: 1.into(),
                jsonrpc: Some("2.0"),
                method,
                params: &Self::convert_params(params),
            })
            .send()
            .await
            .map_err(|err| rpc::error::Error::Transport(Box::new(err)))?;

        let status = res.status();
        let res = if status.is_success() {
            res.json::<rpc::Response>()
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
                .text()
                .await
                .unwrap_or_else(|_| status.canonical_reason().expect("to have a message").into());

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
