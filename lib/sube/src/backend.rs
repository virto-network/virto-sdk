use crate::prelude::*;
use crate::{Backend, Error, Metadata, Result as SubeResult};

use url::Url;

#[cfg(any(feature = "http", feature = "http-web"))]
use crate::http::Backend as HttpBackend;
#[cfg(any(feature = "http", feature = "http-web", feature = "ws", feature = "js"))]
use crate::rpc::RpcClient;
#[cfg(feature = "ws")]
use crate::ws::Backend as WSBackend;

use heapless::index_map::FnvIndexMap as Map;
use no_std_async::Mutex;

// --- Internal backend enum + dispatch ---

pub(crate) enum AnyBackend {
    #[cfg(any(feature = "http", feature = "http-web"))]
    Http(RpcClient<HttpBackend>),
    #[cfg(feature = "ws")]
    Ws(RpcClient<WSBackend>),
}

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.$method($($arg),*).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.$method($($arg),*).await,
            #[allow(unreachable_patterns)]
            _ => unreachable!("no backend available"),
        }
    };
}

#[allow(unused_variables)]
impl Backend for AnyBackend {
    async fn get_storage_items(
        &self,
        keys: Vec<crate::RawKey>,
        block: Option<u32>,
    ) -> SubeResult<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        dispatch!(self, get_storage_items(keys, block))
    }

    async fn get_keys_paged(
        &self,
        from: crate::RawKey,
        size: u16,
        to: Option<crate::RawKey>,
    ) -> SubeResult<Vec<crate::RawKey>> {
        dispatch!(self, get_keys_paged(from, size, to))
    }

    async fn submit(&self, ext: &[u8]) -> SubeResult<()> {
        dispatch!(self, submit(ext))
    }

    async fn metadata(&self) -> SubeResult<Metadata> {
        dispatch!(self, metadata())
    }

    async fn block_info(&self, at: Option<u32>) -> SubeResult<crate::meta::BlockInfo> {
        dispatch!(self, block_info(at))
    }
}

// --- Global metadata cache ---

static META_CACHE: async_once_cell::OnceCell<Mutex<Map<String, &'static Metadata, 16>>> =
    async_once_cell::OnceCell::new();

/// Get or fetch+leak metadata for a given host, keyed by scheme://host:port.
pub(crate) async fn get_metadata(
    backend: &AnyBackend,
    url: &Url,
    preloaded: Option<Metadata>,
) -> SubeResult<&'static Metadata> {
    let key = base_key(url);

    let mut cache = META_CACHE
        .get_or_init(async { Mutex::new(Map::new()) })
        .await
        .lock()
        .await;

    if let Some(&meta) = cache.get(&key) {
        return Ok(meta);
    }

    let meta = match preloaded {
        Some(m) => m,
        None => backend.metadata().await.map_err(|_| Error::BadMetadata)?,
    };
    let meta: &'static Metadata = Box::leak(Box::new(meta));

    cache
        .insert(key, meta)
        .map_err(|_| Error::CantInitBackend)?;

    Ok(meta)
}

fn base_key(url: &Url) -> String {
    format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or("unknown"),
        url.port().unwrap_or(match url.scheme() {
            "wss" | "https" => 443,
            _ => 80,
        })
    )
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
        let port = match url.scheme() {
            "ws" => 9944,
            _ => 9933,
        };
        url.set_port(Some(port)).expect("known port");
    }

    Ok(url)
}

// --- Connect ---

pub(crate) async fn connect(url: &Url) -> SubeResult<AnyBackend> {
    match url.scheme() {
        #[cfg(feature = "ws")]
        "ws" | "wss" => Ok(AnyBackend::Ws(RpcClient(
            WSBackend::new_ws2(url.to_string().as_str()).await?,
        ))),
        #[cfg(any(feature = "http", feature = "http-web"))]
        "http" | "https" => Ok(AnyBackend::Http(RpcClient(HttpBackend::new(url.clone())))),
        _ => Err(Error::BadInput),
    }
}
