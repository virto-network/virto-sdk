use crate::prelude::*;
use crate::{meta::BlockInfo, Backend, Error, Metadata, Result as SubeResult};
use crate::{Offline, RawKey, RawValue};

use core::future::Future;
use url::Url;

#[cfg(any(feature = "http", feature = "http-web"))]
use crate::http::Backend as HttpBackend;
#[cfg(any(feature = "http", feature = "http-web", feature = "ws", feature = "js"))]
use crate::rpc::RpcClient;
#[cfg(feature = "ws")]
use crate::ws::Backend as WSBackend;

use heapless::index_map::FnvIndexMap as Map;
use no_std_async::Mutex;

pub type BoxFuture<'a, T> = core::pin::Pin<Box<dyn Future<Output = T> + 'a>>;

// --- Static caching ---

static INSTANCE_BACKEND: async_once_cell::OnceCell<
    Mutex<Map<String, Mutex<&'static AnyBackend>, 16>>,
> = async_once_cell::OnceCell::new();

static INSTANCE_METADATA: async_once_cell::OnceCell<
    Mutex<Map<String, Mutex<&'static Metadata>, 16>>,
> = async_once_cell::OnceCell::new();

async fn get_metadata(backend: &AnyBackend, metadata: Option<Metadata>) -> SubeResult<Metadata> {
    match metadata {
        Some(m) => Ok(m),
        None => backend.metadata().await.map_err(|_| Error::BadMetadata),
    }
}

pub(crate) async fn get_multi_backend_by_url<'a>(
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
        "{}://{}:{}",
        url.scheme(),
        url.host_str().expect("url to have a host"),
        url.port().unwrap_or(80)
    );

    let cached_b = instance_backend.get(&base_path);
    let cached_m = instance_metadata.get(&base_path);

    match (cached_b, cached_m) {
        (Some(b), Some(m)) => {
            let b = *b.lock().await;
            let m = *m.lock().await;
            Ok((b, m))
        }
        _ => {
            let backend = Box::new(get_backend_by_url(url.clone()).await?);
            let backend = Box::leak::<'static>(backend);

            instance_backend
                .insert(base_path.clone(), Mutex::new(backend))
                .map_err(|_| Error::CantInitBackend)?;

            let metadata = Box::new(get_metadata(backend, metadata).await?);
            let metadata = Box::leak::<'static>(metadata);

            instance_metadata
                .insert(base_path.clone(), Mutex::new(metadata))
                .map_err(|_| Error::BadMetadata)?;

            Ok((backend, metadata))
        }
    }
}

// --- URL parsing ---

pub(crate) fn chain_string_to_url(chain: &str) -> SubeResult<Url> {
    let chain = if !chain.starts_with("ws://")
        && !chain.starts_with("wss://")
        && !chain.starts_with("http://")
        && !chain.starts_with("https://")
    {
        ["wss", chain].join("://")
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

// --- Backend dispatch ---

pub(crate) enum AnyBackend {
    #[cfg(any(feature = "http", feature = "http-web"))]
    Http(RpcClient<HttpBackend>),
    #[cfg(feature = "ws")]
    Ws(RpcClient<WSBackend>),
    _Offline(Offline),
}

async fn get_backend_by_url(url: Url) -> SubeResult<AnyBackend> {
    match url.scheme() {
        #[cfg(feature = "ws")]
        "ws" | "wss" => Ok(AnyBackend::Ws(RpcClient(
            WSBackend::new_ws2(url.to_string().as_str()).await?,
        ))),
        #[cfg(any(feature = "http", feature = "http-web"))]
        "http" | "https" => Ok(AnyBackend::Http(RpcClient(HttpBackend::new(url)))),
        _ => Err(Error::BadInput),
    }
}

impl Backend for &AnyBackend {
    async fn get_storage_items(
        &self,
        keys: Vec<RawKey>,
        block: Option<u32>,
    ) -> crate::Result<impl Iterator<Item = (RawKey, Option<RawValue>)>> {
        let result: Box<dyn Iterator<Item = (RawKey, Option<RawValue>)>> = match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => Box::new(b.get_storage_items(keys, block).await?),
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => Box::new(b.get_storage_items(keys, block).await?),
            AnyBackend::_Offline(b) => Box::new(b.get_storage_items(keys, block).await?),
        };
        Ok(result)
    }

    async fn get_storage_item(
        &self,
        key: RawKey,
        block: Option<u32>,
    ) -> crate::Result<Option<Vec<u8>>> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.get_storage_item(key, block).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.get_storage_item(key, block).await,
            AnyBackend::_Offline(b) => b.get_storage_item(key, block).await,
        }
    }

    async fn get_keys_paged(
        &self,
        from: RawKey,
        size: u16,
        to: Option<RawKey>,
    ) -> crate::Result<Vec<RawKey>> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.get_keys_paged(from, size, to).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.get_keys_paged(from, size, to).await,
            AnyBackend::_Offline(b) => b.get_keys_paged(from, size, to).await,
        }
    }

    async fn metadata(&self) -> SubeResult<Metadata> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.metadata().await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.metadata().await,
            AnyBackend::_Offline(b) => b.metadata().await,
        }
    }

    async fn submit(&self, ext: impl AsRef<[u8]>) -> SubeResult<()> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.submit(ext).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.submit(ext).await,
            AnyBackend::_Offline(b) => b.submit(ext).await,
        }
    }

    async fn block_info(&self, at: Option<u32>) -> SubeResult<BlockInfo> {
        match self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.block_info(at).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.block_info(at).await,
            AnyBackend::_Offline(b) => b.block_info(at).await,
        }
    }
}
