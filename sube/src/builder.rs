#[cfg(any(feature = "http", feature = "http-web"))]
use crate::http::Backend as HttpBackend;
#[cfg(feature = "ws")]
use crate::ws::Backend as WSBackend;
use crate::{
    meta::BlockInfo, Backend, Error, ExtrinsicBody, Metadata, Response, Result as SubeResult,
    Signer, StorageKey,
};
use crate::{prelude::*, Offline, StorageChangeSet};

use alloc::rc::Rc;
use alloc::sync::Arc;
use async_trait::async_trait;

use async_mutex::Mutex;
use core::borrow::Borrow;
use core::{
    future::{Future, IntoFuture},
    marker::PhantomData,
};

// use std::os::macos::raw::stat;

use url::Url;

pub struct SubeBuilder<'a, Body, Signer> {
    url: Option<&'a str>,
    nonce: Option<u64>,
    body: Option<Body>,
    signer: Option<Signer>,
    metadata: Option<Metadata>,
}

impl<'a> Default for SubeBuilder<'a, (), ()> {
    fn default() -> Self {
        SubeBuilder {
            url: None,
            nonce: None,
            body: None,
            signer: None,
            metadata: None,
        }
    }
}

impl<'a> SubeBuilder<'a, (), ()> {
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

    pub fn with_body<B>(self, body: B) -> SubeBuilder<'a, B, ()> {
        SubeBuilder {
            body: Some(body),
            url: self.url,
            nonce: self.nonce,
            signer: self.signer,
            metadata: self.metadata,
        }
    }

    async fn build_query(self) -> SubeResult<Response<'a>> {
        let Self { url, metadata, .. } = self;

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

        // let (backend, meta) = get_cache_by_url(url.clone(), metadata).await?;

        Ok(match path {
            "_meta" => Response::Meta(meta),
            "_meta/registry" => Response::Registry(&meta.types),
            _ => crate::query(&backend, meta, path).await?,
        })
    }
}

impl<'a, B> SubeBuilder<'a, B, ()> {
    pub fn with_signer<S: Signer>(self, signer: impl Into<S>) -> SubeBuilder<'a, B, S> {
        SubeBuilder {
            signer: Some(signer.into()),
            body: self.body,
            metadata: self.metadata,
            nonce: self.nonce,
            url: self.url,
        }
    }
}

impl<'a, B, S> SubeBuilder<'a, B, S>
where
    B: serde::Serialize + core::fmt::Debug,
    S: Signer,
{
    pub fn with_nonce(self, nonce: u64) -> Self {
        Self {
            nonce: Some(nonce),
            ..self
        }
    }

    async fn build_extrinsic(self) -> SubeResult<Response<'a>> {
        let Self {
            url,
            nonce,
            body,
            signer,
            metadata,
            ..
        } = self;

        let url = chain_string_to_url(&url.ok_or(Error::BadInput)?)?;
        let path = url.path();
        let body = body.ok_or(Error::BadInput)?;

        // let (backend, meta) = get_cache_by_url(url.clone(), metadata).await?;

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
            _ => {
                let signer = signer.ok_or(Error::BadInput)?;

                crate::submit(backend, meta, path, ExtrinsicBody { nonce, body }, signer).await?
            }
        })
    }
}

#[inline]
async fn get_cache_by_url<'a>(
    url: Url,
    metadata: Option<Metadata>,
) -> SubeResult<(&'a AnyBackend, &'a Metadata)> {
    let mut instance_backend = INSTANCE_BACKEND
        .get_or_init(async { Mutex::new(Map::new()) })
        .await
        .lock()
        .await;

    let mut instance_metadata = INSTANCE_METADATA
        .get_or_init(async { Mutex::new(Map::new()) })
        .await
        .lock()
        .await;

    let base_path = format!(
        "{}://{}",
        url.scheme(),
        url.host_str().expect("url to have a host")
    );

    let cached_b = instance_backend.get(&base_path);
    let cached_m = instance_metadata.get(&base_path);

    match (cached_b, cached_m) {
        (Some(b), Some(m)) => Ok((b, m)),
        _ => {
            let backend = Box::new(get_backend_by_url(url.clone()).await?);
            let backend = Box::leak::<'static>(backend);

            instance_backend.insert(base_path.clone(), backend);

            let metadata = Box::new(get_metadata(backend, metadata).await?);
            let metadata = Box::leak::<'static>(metadata);

            instance_metadata.insert(base_path.clone(), metadata);

            Ok((backend, metadata))
        }
    }
}

use heapless::FnvIndexMap as Map;

static INSTANCE_BACKEND: async_once_cell::OnceCell<Mutex<Map<String, &'static AnyBackend, 10>>> =
    async_once_cell::OnceCell::new();

static INSTANCE_METADATA: async_once_cell::OnceCell<Mutex<Map<String, &'static Metadata, 10>>> =
    async_once_cell::OnceCell::new();

static BACKEND: async_once_cell::OnceCell<AnyBackend> = async_once_cell::OnceCell::new();
static META: async_once_cell::OnceCell<Metadata> = async_once_cell::OnceCell::new();

pub type BoxFuture<'a, T> = core::pin::Pin<Box<dyn Future<Output = T> + 'a>>;

impl<'a> IntoFuture for SubeBuilder<'a, (), ()> {
    type Output = SubeResult<Response<'a>>;
    type IntoFuture = BoxFuture<'a, SubeResult<Response<'a>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.build_query())
    }
}

impl<'a, B, S> IntoFuture for SubeBuilder<'a, B, S>
where
    B: serde::Serialize + core::fmt::Debug + 'a,
    S: Signer + 'a,
{
    type Output = SubeResult<Response<'a>>;
    type IntoFuture = BoxFuture<'a, SubeResult<Response<'a>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.build_extrinsic())
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
        #[cfg(any(feature = "http", feature = "http-web"))]
        "http" | "https" => Ok(AnyBackend::Http(HttpBackend::new(url))),
        _ => Err(Error::BadInput),
    }
}

enum AnyBackend {
    #[cfg(any(feature = "http", feature = "http-web"))]
    Http(HttpBackend),
    #[cfg(feature = "ws")]
    Ws(WSBackend),
    Offline(Offline),
}

#[async_trait]
impl Backend for &AnyBackend {
    async fn query_storage_at(
        &self,
        keys: Vec<String>,
        block: Option<String>,
    ) -> crate::Result<Vec<StorageChangeSet>> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.query_storage_at(keys, block).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.query_storage_at(keys, block).await,
            AnyBackend::Offline(b) => b.query_storage_at(keys, block).await,
        }
    }

    async fn get_keys_paged(
        &self,
        from: &StorageKey,
        size: u16,
        to: Option<&StorageKey>,
    ) -> crate::Result<Vec<String>> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.get_keys_paged(&from, size, to).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.get_keys_paged(&from, size, to).await,
            AnyBackend::Offline(b) => b.get_keys_paged(&from, size, to).await,
        }
    }

    async fn metadata(&self) -> SubeResult<Metadata> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.metadata().await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.metadata().await,
            AnyBackend::Offline(b) => b.metadata().await,
        }
    }

    async fn submit<U: AsRef<[u8]> + Send>(&self, ext: U) -> SubeResult<()> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.submit(ext).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.submit(ext).await,
            AnyBackend::Offline(b) => b.submit(ext).await,
        }
    }

    async fn block_info(&self, at: Option<u32>) -> SubeResult<BlockInfo> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.block_info(at).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.block_info(at).await,
            AnyBackend::Offline(b) => b.block_info(at).await,
        }
    }

    async fn query_storage(&self, key: &StorageKey) -> SubeResult<Vec<u8>> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.query_storage(key).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.query_storage(key).await,
            AnyBackend::Offline(b) => b.query_storage(key).await,
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
            $crate::SubeBuilder::default().with_url($url).await
        }
    };

    ($url:expr => ($wallet:expr, $body:expr)) => {
        async {
            let mut builder = $crate::SubeBuilder::default();

            let public = $wallet.default_account().expect("to have a default account").public();

            builder
                .with_url($url)
                .with_body($body)
                .with_signer($crate::SignerFn::from((public, |message: &[u8]| async { Ok($wallet.sign(message).await?) })))
                .await?;

            $crate::Result::Ok($crate::Response::Void)
        }
    };
}
