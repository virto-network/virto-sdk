use core::time::Duration;

use alloc::sync::Arc;

use crate::prelude::*;
use crate::url::Url;
use crate::{Backend, Error, Metadata, Result as SubeResult};

#[cfg(any(feature = "ws", feature = "smoldot"))]
use crate::rpc::chainhead::ChainHead;

#[cfg(all(feature = "smoldot", feature = "std"))]
type SmoldotPlatform = alloc::sync::Arc<smoldot_light::platform::DefaultPlatform>;

use core::fmt::Write;
use heapless::index_map::FnvIndexMap as Map;
use no_std_async::Mutex;

type CacheKey = heapless::String<64>;

// --- Internal backend enum + dispatch ---

pub(crate) enum AnyBackend {
    #[cfg(feature = "ws")]
    Ws(Box<ChainHead<crate::rpc::ws::Backend>>),
    #[cfg(all(feature = "smoldot", feature = "std"))]
    Smoldot(Box<ChainHead<crate::rpc::smoldot::Backend<SmoldotPlatform>>>),
}

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
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
        &mut self,
        keys: Vec<crate::RawKey>,
        block: Option<u32>,
    ) -> SubeResult<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        dispatch!(self, get_storage_items(keys, block))
    }

    async fn get_keys_paged(
        &mut self,
        from: crate::RawKey,
        size: u16,
        to: Option<crate::RawKey>,
    ) -> SubeResult<Vec<crate::RawKey>> {
        dispatch!(self, get_keys_paged(from, size, to))
    }

    async fn submit(&mut self, ext: &[u8]) -> SubeResult<()> {
        dispatch!(self, submit(ext))
    }

    async fn metadata(&mut self) -> SubeResult<Metadata> {
        dispatch!(self, metadata())
    }

    async fn block_info(&mut self, at: Option<u32>) -> SubeResult<crate::meta::BlockInfo> {
        dispatch!(self, block_info(at))
    }
}

// --- Chain event forwarding ---

#[cfg(any(feature = "ws", feature = "smoldot"))]
impl AnyBackend {
    pub(crate) async fn next_chain_event(
        &mut self,
    ) -> SubeResult<crate::rpc::chainhead::ChainEvent> {
        dispatch!(self, next_chain_event())
    }

    pub(crate) async fn runtime_call_at(
        &mut self,
        block_hash: &str,
        function: &str,
        call_data: &str,
    ) -> SubeResult<Vec<u8>> {
        dispatch!(self, runtime_call_at(block_hash, function, call_data))
    }

    pub(crate) async fn header(
        &mut self,
        block_hash: &str,
    ) -> SubeResult<crate::rpc::chainhead::BlockHeader> {
        dispatch!(self, header(block_hash))
    }

    pub(crate) async fn get_storage_at_hash(
        &mut self,
        block_hash: &str,
        keys: Vec<crate::RawKey>,
    ) -> SubeResult<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        dispatch!(self, get_storage_at_hash(block_hash, keys))
    }

    pub(crate) fn try_next_chain_event(&mut self) -> Option<crate::rpc::chainhead::ChainEvent> {
        match self {
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.try_next_chain_event(),
            #[cfg(all(feature = "smoldot", feature = "std"))]
            AnyBackend::Smoldot(b) => b.try_next_chain_event(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

// --- Global metadata cache ---

static META_CACHE: Mutex<Option<Map<CacheKey, Arc<Metadata>, 16>>> = Mutex::new(None);

/// Get or fetch metadata, keyed by scheme://host:port.
pub(crate) async fn get_metadata(
    backend: &mut AnyBackend,
    url: &Url,
    preloaded: Option<Metadata>,
) -> SubeResult<Arc<Metadata>> {
    let key = base_key(url).map_err(|_| Error::BadInput)?;
    get_or_fetch(key, backend, preloaded).await
}

/// Fetch+cache metadata using a string cache key (for backends without a URL).
#[cfg(all(feature = "smoldot", feature = "std"))]
pub(crate) async fn get_metadata_by_key(
    backend: &mut AnyBackend,
    cache_key: &str,
    preloaded: Option<Metadata>,
) -> SubeResult<Arc<Metadata>> {
    let key: CacheKey = cache_key.try_into().map_err(|_| Error::BadInput)?;
    get_or_fetch(key, backend, preloaded).await
}

async fn get_or_fetch(
    key: CacheKey,
    backend: &mut AnyBackend,
    preloaded: Option<Metadata>,
) -> SubeResult<Arc<Metadata>> {
    let mut cache = META_CACHE.lock().await;
    let map = cache.get_or_insert_with(Map::new);

    if let Some(meta) = map.get(&key) {
        return Ok(Arc::clone(meta));
    }

    let meta = match preloaded {
        Some(m) => m,
        None => backend.metadata().await.map_err(|_| Error::BadMetadata)?,
    };
    let meta = Arc::new(meta);

    map.insert(key, Arc::clone(&meta))
        .map_err(|_| Error::BadMetadata)?;

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

// --- Timeout ---

/// Race a future against a timer. When `ws` or `smoldot` features are enabled
/// `smol::Timer` is available; otherwise the timeout is a no-op.
#[cfg(any(feature = "ws", feature = "smoldot-std"))]
async fn with_timeout<T>(
    duration: Duration,
    fut: impl core::future::Future<Output = SubeResult<T>>,
) -> SubeResult<T> {
    use core::pin::pin;
    use futures_util::future::Either;
    match futures_util::future::select(pin!(fut), pin!(smol::Timer::after(duration))).await {
        Either::Left((result, _)) => result,
        Either::Right((_, _)) => Err(Error::ConnectionTimeout),
    }
}

#[cfg(not(any(feature = "ws", feature = "smoldot-std")))]
async fn with_timeout<T>(
    _duration: Duration,
    fut: impl core::future::Future<Output = SubeResult<T>>,
) -> SubeResult<T> {
    fut.await
}

// --- Connect ---

pub(crate) async fn connect(url: &Url, timeout: Duration) -> SubeResult<AnyBackend> {
    with_timeout(timeout, async {
        match url.scheme() {
            #[cfg(feature = "ws")]
            "ws" | "wss" => {
                let ws = crate::rpc::ws::Backend::new(url.to_string().as_str())
                    .await
                    .map_err(|e| Error::Node(format!("connecting to {url}: {e}")))?;
                let chainhead = ChainHead::new(ws)
                    .await
                    .map_err(|e| Error::Node(format!("chain session for {url}: {e}")))?;
                Ok(AnyBackend::Ws(Box::new(chainhead)))
            }
            _ => Err(Error::BadInput),
        }
    })
    .await
}

/// Connect via smoldot light client using a chain spec (std only).
#[cfg(all(feature = "smoldot", feature = "std"))]
pub(crate) async fn connect_light(chain_spec: &str, timeout: Duration) -> SubeResult<AnyBackend> {
    with_timeout(timeout, async {
        let backend = crate::rpc::smoldot::Backend::new_std(chain_spec)?;
        let chainhead = ChainHead::new(backend).await?;
        Ok(AnyBackend::Smoldot(Box::new(chainhead)))
    })
    .await
}

/// Connect via smoldot light client for a parachain (std only).
#[cfg(all(feature = "smoldot", feature = "std"))]
pub(crate) async fn connect_light_para(
    chain_spec: &str,
    relay_spec: &str,
    timeout: Duration,
) -> SubeResult<AnyBackend> {
    with_timeout(timeout, async {
        let backend =
            crate::rpc::smoldot::Backend::new_std_with_relay(chain_spec, Some(relay_spec))?;
        let chainhead = ChainHead::new(backend).await?;
        Ok(AnyBackend::Smoldot(Box::new(chainhead)))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_string_with_scheme() {
        let url = chain_string_to_url("wss://kreivo.io").unwrap();
        assert_eq!(url.scheme(), "wss");
        assert_eq!(url.host_str(), Some("kreivo.io"));
    }

    #[test]
    fn chain_string_without_scheme_defaults_to_wss() {
        let url = chain_string_to_url("kreivo.io").unwrap();
        assert_eq!(url.scheme(), "wss");
        assert_eq!(url.host_str(), Some("kreivo.io"));
    }

    #[test]
    fn chain_string_ws_localhost_default_port() {
        let url = chain_string_to_url("ws://localhost").unwrap();
        assert_eq!(url.port(), Some(9944));
    }

    #[test]
    fn chain_string_http_localhost_default_port() {
        let url = chain_string_to_url("http://localhost").unwrap();
        assert_eq!(url.port(), Some(9933));
    }

    #[test]
    fn chain_string_preserves_port_and_path() {
        let url = chain_string_to_url("wss://example.com:1234/some/path").unwrap();
        assert_eq!(url.port(), Some(1234));
        assert_eq!(url.path(), "/some/path");
    }
}
