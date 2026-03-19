use crate::prelude::*;
use crate::url::Url;
use crate::{Backend, Error, Metadata, Result as SubeResult};

#[cfg(any(feature = "http", feature = "http-web"))]
use crate::http::Backend as HttpBackend;
#[cfg(any(
    feature = "http",
    feature = "http-web",
    feature = "ws",
    feature = "js",
    feature = "smoldot"
))]
use crate::rpc::RpcClient;
#[cfg(feature = "ws")]
use crate::ws::Backend as WSBackend;

#[cfg(all(feature = "smoldot", feature = "std"))]
type SmoldotPlatform = alloc::sync::Arc<smoldot_light::platform::DefaultPlatform>;

use core::fmt::Write;
use heapless::index_map::FnvIndexMap as Map;
use no_std_async::Mutex;

type CacheKey = heapless::String<64>;

// --- Internal backend enum + dispatch ---

pub(crate) enum AnyBackend {
    #[cfg(any(feature = "http", feature = "http-web"))]
    Http(RpcClient<HttpBackend>),
    #[cfg(feature = "ws")]
    Ws(RpcClient<WSBackend>),
    #[cfg(all(feature = "smoldot", feature = "std"))]
    Smoldot(RpcClient<crate::smoldot::Backend<SmoldotPlatform>>),
}

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            #[cfg(any(feature = "http", feature = "http-web"))]
            AnyBackend::Http(b) => b.$method($($arg),*).await,
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.$method($($arg),*).await,
            #[cfg(all(feature = "smoldot", feature = "std"))]
            AnyBackend::Smoldot(b) => b.$method($($arg),*).await,
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

static META_CACHE: Mutex<Option<Map<CacheKey, &'static Metadata, 16>>> = Mutex::new(None);

/// Get or fetch+leak metadata, keyed by scheme://host:port.
pub(crate) async fn get_metadata(
    backend: &AnyBackend,
    url: &Url,
    preloaded: Option<Metadata>,
) -> SubeResult<&'static Metadata> {
    let key = base_key(url).map_err(|_| Error::BadInput)?;
    get_or_fetch(key, backend, preloaded).await
}

/// Fetch+cache metadata using a string cache key (for backends without a URL).
#[cfg(all(feature = "smoldot", feature = "std"))]
pub(crate) async fn get_metadata_by_key(
    backend: &AnyBackend,
    cache_key: &str,
    preloaded: Option<Metadata>,
) -> SubeResult<&'static Metadata> {
    let key: CacheKey = cache_key.try_into().map_err(|_| Error::BadInput)?;
    get_or_fetch(key, backend, preloaded).await
}

async fn get_or_fetch(
    key: CacheKey,
    backend: &AnyBackend,
    preloaded: Option<Metadata>,
) -> SubeResult<&'static Metadata> {
    let mut cache = META_CACHE.lock().await;
    let map = cache.get_or_insert_with(Map::new);

    if let Some(&meta) = map.get(&key) {
        return Ok(meta);
    }

    let meta = match preloaded {
        Some(m) => m,
        None => backend.metadata().await.map_err(|_| Error::BadMetadata)?,
    };
    let meta: &'static Metadata = Box::leak(Box::new(meta));

    map.insert(key, meta).map_err(|_| Error::BadMetadata)?;

    Ok(meta)
}

fn base_key(url: &Url) -> core::result::Result<CacheKey, core::fmt::Error> {
    let mut key = CacheKey::new();
    let port = url.port().unwrap_or(match url.scheme() {
        "wss" | "https" => 443,
        _ => 80,
    });
    write!(
        key,
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or("unknown"),
        port
    )?;
    Ok(key)
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

    if url.host_str() == Some("localhost") && url.port().is_none() {
        let port = match url.scheme() {
            "ws" => 9944,
            _ => 9933,
        };
        url.set_port(Some(port));
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
        "http" | "https" => Ok(AnyBackend::Http(RpcClient(HttpBackend::new(
            url.to_string(),
        )))),
        _ => Err(Error::BadInput),
    }
}

/// Connect via smoldot light client using a chain spec (std only).
#[cfg(all(feature = "smoldot", feature = "std"))]
pub(crate) fn connect_light(chain_spec: &str) -> SubeResult<AnyBackend> {
    let backend = crate::smoldot::Backend::new_std(chain_spec)?;
    Ok(AnyBackend::Smoldot(RpcClient(backend)))
}

/// Connect via smoldot light client for a parachain (std only).
#[cfg(all(feature = "smoldot", feature = "std"))]
pub(crate) fn connect_light_para(chain_spec: &str, relay_spec: &str) -> SubeResult<AnyBackend> {
    let backend = crate::smoldot::Backend::new_std_with_relay(chain_spec, Some(relay_spec))?;
    Ok(AnyBackend::Smoldot(RpcClient(backend)))
}
