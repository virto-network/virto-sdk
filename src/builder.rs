use crate::http::Backend as HttpBackend;
use crate::prelude::*;
use crate::ws::{Backend as WSbackend, WS2};
use crate::{
    meta::BlockInfo, Backend, Error, ExtrinicBody, Metadata, Response, Result as SubeResult,
    StorageKey,
};

use async_trait::async_trait;
use core::future::{Future, IntoFuture};
use url::Url;

pub trait SignerFn: Fn(&[u8], &mut [u8; 64]) -> SubeResult<()> {}
impl<T> SignerFn for T where T: Fn(&[u8], &mut [u8; 64]) -> SubeResult<()> {}

pub struct SubeBuilder<'a, Body, F>
where
    Body: serde::Serialize,
{
    url: Option<&'a str>,
    nonce: Option<u64>,
    body: Option<ExtrinicBody<Body>>,
    sender: Option<&'a [u8]>,
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
            sender: None,
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
    pub fn with_url(self, url: &'a str) -> Self {
        Self {
            url: Some(url),
            ..self
        }
    }

    pub fn with_body(self, body: ExtrinicBody<Body>) -> Self {
        Self {
            body: Some(body),
            ..self
        }
    }

    pub fn with_signer(self, signer: F) -> Self {
        Self {
            signer: Some(signer),
            ..self
        }
    }

    pub fn with_nonce(self, nonce: u64) -> Self {
        Self {
            nonce: Some(nonce),
            ..self
        }
    }

    pub fn with_meta(self, metadata: Metadata) -> Self {
        Self {
            metadata: Some(metadata),
            ..self
        }
    }

    pub fn with_sender(self, sender: &'a [u8]) -> Self {
        Self {
            sender: Some(sender),
            ..self
        }
    }
}

static BACKEND: async_once_cell::OnceCell<AnyBackend> = async_once_cell::OnceCell::new();
static META: async_once_cell::OnceCell<Metadata> = async_once_cell::OnceCell::new();

impl<'a, Body, F> IntoFuture for SubeBuilder<'a, Body, F>
where
    Body: serde::Serialize,
    F: SignerFn,
{
    type Output = SubeResult<Response<'a>>;
    type IntoFuture = impl Future<Output = Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let Self {
            url,
            nonce: _,
            body,
            sender: _,
            signer,
            metadata,
        } = self;
        async move {
            let url = chain_string_to_url(&url.ok_or(Error::BadInput)?)?;
            let backend = BACKEND
                .get_or_try_init(get_backend_by_url(url.clone()))
                .await?;

            let meta = META
                .get_or_try_init(async {
                    match metadata {
                        Some(m) => Ok(m),
                        None => backend.metadata().await.map_err(|_| Error::BadMetadata),
                    }
                })
                .await?;

            let signer = signer.ok_or(Error::BadInput)?;

            let path = url.path();
            Ok(match path {
                "_meta" => Response::Meta(meta),
                "_meta/registry" => Response::Registry(&meta.types),
                _ => {
                    if let Some(tx_data) = body {
                        crate::submit(backend, meta, path, tx_data, signer).await?
                    } else {
                        crate::query(&backend, meta, path).await?
                    }
                }
            })
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

async fn get_backend_by_url(url: Url) -> SubeResult<AnyBackend> {
    match url.scheme() {
        "ws" | "wss" => Ok(AnyBackend::Ws(
            WSbackend::new_ws2(url.to_string().as_str()).await?,
        )),
        "http" | "https" => Ok(AnyBackend::Http(HttpBackend::new(url))),
        _ => Err(Error::BadInput),
    }
}

enum AnyBackend {
    Http(HttpBackend),
    Ws(WSbackend<WS2>),
}

#[async_trait]
impl Backend for &AnyBackend {
    async fn metadata(&self) -> SubeResult<Metadata> {
        match self {
            AnyBackend::Http(b) => b.metadata().await,
            AnyBackend::Ws(b) => b.metadata().await,
        }
    }

    async fn submit<U: AsRef<[u8]> + Send>(&self, ext: U) -> SubeResult<()> {
        match self {
            AnyBackend::Http(b) => b.submit(ext).await,
            AnyBackend::Ws(b) => b.submit(ext).await,
        }
    }

    async fn block_info(&self, at: Option<u32>) -> SubeResult<BlockInfo> {
        match self {
            AnyBackend::Http(b) => b.block_info(at).await,
            AnyBackend::Ws(b) => b.block_info(at).await,
        }
    }
    async fn query_storage(&self, key: &StorageKey) -> SubeResult<Vec<u8>> {
        match self {
            AnyBackend::Http(b) => b.query_storage(key).await,
            AnyBackend::Ws(b) => b.query_storage(&key).await,
        }
    }
}
