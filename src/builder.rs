use crate::http::Backend as HttpBackend;
use crate::prelude::*;
use crate::rpc::RpcResult;
use crate::ws::{Backend as WSbackend, WS2};
use crate::{
    exec, meta::BlockInfo, rpc, Backend, Error, ExtrinicBody, Metadata, Response,
    Result as SubeResult, StorageKey,
};

use async_trait::async_trait;
use core::any::Any;
use core::future::{Future, IntoFuture};
use core::pin::Pin;
use scale_info::build;
use serde::Serializer;
use url::Url;

pub trait SignerFn: Fn(&[u8], &mut [u8; 64]) -> SubeResult<()> {}
impl<T> SignerFn for T where T: Fn(&[u8], &mut [u8; 64]) -> SubeResult<()> {}
// pub type SignerFn = impl Fn(&[u8], &mut [u8; 64]) -> SubeResult<()>;

pub struct SubeBuilder<'a, Body, F>
where
    Body: serde::Serialize,
{
    url: Option<&'a str>,
    nonce: Option<u64>,
    body: Option<ExtrinicBody<Body>>,
    address: Option<&'a [u8]>,
    signer: Option<F>,
    metadata: Option<Metadata>,
}

impl<'a, Body, F> Default for SubeBuilder<'a, Body, F>
where
    Body: serde::Serialize,
{
    fn default() -> Self {
        SubeBuilder {
            url: None,
            nonce: None,
            body: None,
            address: None,
            signer: None,
            metadata: None,
        }
    }
}

impl<'a, Body, F> SubeBuilder<'a, Body, F>
where
    Body: serde::Serialize,
    F: SignerFn,
{
    // pub fn new(url: &'a str) -> Self {
    //     SubeBuilder { url, ..Default::default() }
    // }

    // pub fn call_mut<U: Into<&'a str>>(url: U) -> &'a mut Self {
    //     let mut builder = SubeBuilder {
    //         url: Some(url.into()),
    //         nonce: None,
    //         body: None,
    //         address: None,
    //         signer: None,
    //         metadata: None,
    //     };

    //     builder.set_url(url.into());

    //     &mut builder
    // }

    fn with_url(self, url: &str) -> Self {
        Self {
            url: Some(url),
            ..self
        }
    }

    fn with_body(self, body: ExtrinicBody<Body>) -> Self {
        Self {
            body: Some(body),
            ..self
        }
    }

    fn with_signer(self, signer: F) -> Self {
        Self {
            signer: Some(signer),
            ..self
        }
    }

    fn with_nonce(self, nonce: u64) -> Self {
        Self {
            nonce: Some(nonce),
            ..self
        }
    }

    fn with_meta(self, metadata: Metadata) -> Self {
        Self {
            metadata: Some(metadata),
            ..self
        }
    }

    fn with_from(self, address: &[u8]) -> Self {
        Self {
            address: Some(address),
            ..self
        }
    }
}

fn chain_string_to_url(chain: &str) -> SubeResult<Url> {
    let chain = if !chain.starts_with("ws://")
        && !chain.starts_with("wss://")
        && !chain.starts_with("http://")
        && !chain.starts_with("https://")
    {
        ["wss", &chain].join("://")
    } else {
        chain.into()
    };

    let mut url = Url::parse(&chain).map_err(|_| Error::BadInput)?;

    if url.host_str().eq(&Some("localhost")) && url.port().is_none() {
        const WS_PORT: u16 = 9944;
        const HTTP_PORT: u16 = 9933;
        let port = match url.scheme() {
            "ws" => WS_PORT,
            _ => HTTP_PORT,
        };

        url.set_port(Some(port)).expect("known port");
    }

    Ok(url)
}

// write an enum called AnyBackend which holds the two types of backend and implements the Backend trait
// this is a hack to make the compiler happy

enum AnyBackend {
    http(HttpBackend),
    ws(WSbackend<WS2>),
}

#[async_trait]
impl Backend for AnyBackend {
    async fn metadata(&self) -> SubeResult<Metadata> {
        match self {
            AnyBackend::http(b) => b.metadata().await,
            AnyBackend::ws(b) => b.metadata().await,
        }
    }

    async fn submit<U: AsRef<[u8]> + Send>(&self, ext: U) -> SubeResult<()> {
        match self {
            AnyBackend::http(b) => b.submit(ext).await,
            AnyBackend::ws(b) => b.submit(ext).await,
        }
    }

    async fn block_info(&self, at: Option<u32>) -> SubeResult<BlockInfo> {
        match self {
            AnyBackend::http(b) => b.block_info(at).await,
            AnyBackend::ws(b) => b.block_info(at).await,
        }
    }
    async fn query_storage(&self, key: &StorageKey) -> SubeResult<Vec<u8>> {
        match self {
            AnyBackend::http(b) => b.query_storage(key).await,
            AnyBackend::ws(b) => b.query_storage(&key).await,
        }
    }
}

async fn get_backend_by_url(url: Url) -> SubeResult<AnyBackend> {
    match url.scheme() {
        "ws" | "wss" => Ok(AnyBackend::ws(
            WSbackend::new_ws2(url.to_string().as_str()).await?,
        )),
        "http" | "https" => Ok(AnyBackend::http(HttpBackend::new(url))),
        _ => Err(Error::BadInput),
    }
}
impl<'a, Body, F> IntoFuture for SubeBuilder<'a, Body, F>
where
    Body: serde::Serialize,
    F: SignerFn,
{
    // type Output = SubeResult<Response<'m>>;
    type Output = SubeResult<()>;
    type IntoFuture = impl Future<Output = Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Ok(async move {
            let url = chain_string_to_url(&self.url.ok_or(Error::BadInput)?)?;
            let backend = get_backend_by_url(url).await?;

            let meta = match self.metadata {
                Some(m) => m,
                None => backend.metadata().await.map_err(|e| Error::BadMetadata)?,
            };

            let signer = self.signer.unwrap_or(|_a, _b| Ok(()));
            if self.signer.is_none() {
                return Ok(exec(
                    backend,
                    &meta,
                    self.url.expect("url must be defined"),
                    None,
                    signer,
                ));
            }

            Ok(exec(
                backend,
                &meta,
                self.url.expect("url must be defined"),
                Some(self.body.expect("to have a body")),
                signer,
            ))
        })
    }
}
