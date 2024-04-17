use crate::http::Backend as HttpBackend;

#[cfg(feature = "ws")]
use crate::ws::Backend as WSBackend;

use crate::meta::Meta;
use crate::prelude::*;
use crate::{
    meta::BlockInfo, Backend, Error, ExtrinsicBody, Metadata, Response, Result as SubeResult,
    SignerFn, StorageKey,
};

use async_trait::async_trait;
use core::future::{Future, IntoFuture};
use core::pin::Pin;
use heapless::Vec as HVec;
use once_cell::sync::OnceCell;
use scale_info::build;
use serde::Serializer;
use url::Url;

type PairHostBackend<'a> = (&'a str, AnyBackend, Metadata);
static INSTANCE: OnceCell<HVec<PairHostBackend, 10>> = OnceCell::new();

pub struct SubeBuilder<'a, Body, Signer> {
    url: Option<&'a str>,
    nonce: Option<u64>,
    body: Option<Body>,
    sender: Option<&'a [u8]>,
    signer: Option<Signer>,
    metadata: Option<Metadata>,
}

impl<'a, B> Default for SubeBuilder<'a, B, ()> {
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

impl<'a, B> SubeBuilder<'a, B, ()> {
    pub fn with_url(self, url: &'a str) -> Self {
        Self {
            url: Some(url),
            ..self
        }
    }

    pub fn with_meta(self, metadata: Metadata) -> Self {
        Self {
            metadata: Some(metadata),
            ..self
        }
    }
}

impl<'a, B> SubeBuilder<'a, B, ()> {
    pub fn with_signer<Signer: SignerFn>(self, signer: Signer) -> SubeBuilder<'a, B, Signer> {
        SubeBuilder {
            signer: Some(signer),
            body: self.body,
            metadata: self.metadata,
            nonce: self.nonce,
            sender: self.sender,
            url: self.url,
        }
    }
}

impl<'a, B, S> SubeBuilder<'a, B, S>
where
    B: serde::Serialize,
    S: SignerFn,
{
    pub fn with_body(self, body: B) -> Self {
        Self {
            body: Some(body),
            ..self
        }
    }

    pub fn with_nonce(self, nonce: u64) -> Self {
        Self {
            nonce: Some(nonce),
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

impl<'a> IntoFuture for SubeBuilder<'a, (), ()> {
    type Output = SubeResult<Response<'a>>;
    type IntoFuture = impl Future<Output = Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let Self { url, metadata, .. } = self;

        async move {
            let url = chain_string_to_url(&url.ok_or(Error::BadInput)?)?;
            let path = url.path();

            log::info!("building the backend for {}", url);
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

            Ok(match path {
                "_meta" => Response::Meta(meta),
                "_meta/registry" => Response::Registry(&meta.types),
                _ => crate::query(&backend, meta, path).await?,
            })
        }
    }
}

impl<'a, Body, Signer> IntoFuture for SubeBuilder<'a, Body, Signer>
where
    Body: serde::Serialize + core::fmt::Debug,
    Signer: SignerFn,
{
    type Output = SubeResult<Response<'a>>;
    type IntoFuture = impl Future<Output = Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let Self {
            url,
            nonce,
            body,
            sender,
            signer,
            metadata,
        } = self;

        async move {
            let url = chain_string_to_url(&url.ok_or(Error::BadInput)?)?;
            let path = url.path();
            let body = body.ok_or(Error::BadInput)?;

            let backend = BACKEND
                .get_or_try_init(get_backend_by_url(url.clone()))
                .await?;

            let meta = META
                .get_or_try_init(async {
                    match metadata {
                        Some(m) => Ok(m),
                        None => backend.metadata().await.map_err(|err| Error::BadMetadata),
                    }
                })
                .await?;

            Ok(match path {
                "_meta" => Response::Meta(meta),
                "_meta/registry" => Response::Registry(&meta.types),
                _ => {
                    let signer = signer.ok_or(Error::BadInput)?;
                    let from = sender.ok_or(Error::BadInput)?;

                    crate::submit(
                        backend,
                        meta,
                        path,
                        from,
                        ExtrinsicBody { nonce, body },
                        signer,
                    )
                    .await?
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
        #[cfg(feature = "ws")]
        "ws" | "wss" => Ok(AnyBackend::Ws(
            #[cfg(feature = "ws")]
            WSBackend::new_ws2(url.to_string().as_str()).await?,
        )),
        "http" | "https" => Ok(AnyBackend::Http(HttpBackend::new(url))),
        _ => Err(Error::BadInput),
    }
}

enum AnyBackend {
    Http(HttpBackend),
    #[cfg(feature = "ws")]
    Ws(WSBackend),
}

#[async_trait]
impl Backend for &AnyBackend {
    async fn metadata(&self) -> SubeResult<Metadata> {
        match self {
            AnyBackend::Http(b) => b.metadata().await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.metadata().await,
        }
    }

    async fn submit<U: AsRef<[u8]> + Send>(&self, ext: U) -> SubeResult<()> {
        match self {
            AnyBackend::Http(b) => b.submit(ext).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.submit(ext).await,
        }
    }

    async fn block_info(&self, at: Option<u32>) -> SubeResult<BlockInfo> {
        match self {
            AnyBackend::Http(b) => b.block_info(at).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.block_info(at).await,
        }
    }
    async fn query_storage(&self, key: &StorageKey) -> SubeResult<Vec<u8>> {
        match self {
            AnyBackend::Http(b) => b.query_storage(key).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.query_storage(&key).await,
        }
    }
}

#[inline]
async fn get_metadata(b: &AnyBackend, metadata: Option<Metadata>) -> SubeResult<Metadata> {
    match metadata {
        Some(m) => Ok(m),
        None => Ok(b.metadata().await?),
    }
}

#[macro_export]
macro_rules! sube {

    ($url:expr) => {
        async {
            $crate::builder::SubeBuilder::default().with_url($url).await
        }
    };

    // Two parameters
    // Match when the macro is called with an expression (url) followed by a block of key-value pairs
    ( $url:expr => { $($key:ident: $value:expr),+ $(,)? }) => {

        async {
            use $crate::paste;

            let mut builder = $crate::builder::SubeBuilder::default()
                .with_url($url);

            paste!($(
                let mut builder = builder.[<with_ $key>]($value);
            )*);

            builder.await
        }
    };

    ($url:expr => ($wallet:expr, $body:expr)) => {
        async {
            let mut builder = $crate::builder::SubeBuilder::default();

            let public = $wallet.default_account().public();

            builder
                .with_url($url)
                .with_signer(|message: &[u8]| Ok($wallet.sign(message).as_bytes()))
                .with_sender(&public.as_ref())
                .with_body($body)
                .await?;

            $crate::Result::Ok($crate::Response::Void)
        }
    };
}
