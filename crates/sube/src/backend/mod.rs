use core::time::Duration;

use alloc::rc::Rc;

use crate::prelude::*;
use crate::rpc::chainhead::ChainHead;
use crate::{Backend, Error, Metadata, Result as SubeResult};

mod url;
use url::Url;

#[cfg(all(feature = "smoldot", feature = "std"))]
type SmoldotPlatform = crate::rpc::managed_platform::ManagedPlatform;

// --- Internal backend enum + dispatch ---

pub enum AnyBackend {
    #[cfg(feature = "ws")]
    Ws(Box<ChainHead<crate::rpc::ws::Backend>>),
    #[cfg(all(feature = "ws-web", target_arch = "wasm32"))]
    WsWeb(Box<ChainHead<crate::rpc::ws_web::Backend>>),
    #[cfg(all(feature = "smoldot", feature = "std"))]
    Smoldot(Box<ChainHead<crate::rpc::smoldot::Backend<SmoldotPlatform>>>),
}

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.$method($($arg),*).await,
            #[cfg(all(feature = "ws-web", target_arch = "wasm32"))]
            AnyBackend::WsWeb(b) => b.$method($($arg),*).await,
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

    async fn get_storage_items_at_hash(
        &mut self,
        keys: Vec<crate::RawKey>,
        block_hash: [u8; 32],
    ) -> SubeResult<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        dispatch!(self, get_storage_items_at_hash(keys, block_hash))
    }

    async fn get_keys_paged(
        &mut self,
        from: crate::RawKey,
        size: u16,
        to: Option<crate::RawKey>,
    ) -> SubeResult<Vec<crate::RawKey>> {
        dispatch!(self, get_keys_paged(from, size, to))
    }

    async fn get_keys_paged_at(
        &mut self,
        prefix: crate::RawKey,
        size: u16,
        start_key: Option<crate::RawKey>,
        block: Option<u32>,
    ) -> SubeResult<Vec<crate::RawKey>> {
        dispatch!(self, get_keys_paged_at(prefix, size, start_key, block))
    }

    async fn get_keys_paged_at_hash(
        &mut self,
        prefix: crate::RawKey,
        size: u16,
        start_key: Option<crate::RawKey>,
        block_hash: [u8; 32],
    ) -> SubeResult<Vec<crate::RawKey>> {
        dispatch!(
            self,
            get_keys_paged_at_hash(prefix, size, start_key, block_hash)
        )
    }

    async fn get_keys_page_at_hash(
        &mut self,
        prefix: crate::RawKey,
        limit: u16,
        cursor: Option<crate::RawKey>,
        block_hash: [u8; 32],
    ) -> SubeResult<crate::RawKeysPage> {
        dispatch!(
            self,
            get_keys_page_at_hash(prefix, limit, cursor, block_hash)
        )
    }

    async fn submit(&mut self, ext: &[u8], wait_for_finalization: bool) -> SubeResult<()> {
        dispatch!(self, submit(ext, wait_for_finalization))
    }

    async fn submit_transaction(
        &mut self,
        ext: &crate::EncodedExtrinsic,
        wait_for: crate::WaitFor,
    ) -> SubeResult<crate::TransactionReceipt> {
        dispatch!(self, submit_transaction(ext, wait_for))
    }

    async fn inspect_transaction(
        &mut self,
        ext: &crate::EncodedExtrinsic,
    ) -> SubeResult<crate::TransactionReport> {
        dispatch!(self, inspect_transaction(ext))
    }

    async fn enrich_receipt(
        &mut self,
        receipt: crate::TransactionReceipt,
        metadata: &Metadata,
    ) -> SubeResult<crate::TransactionReceipt> {
        dispatch!(self, enrich_receipt(receipt, metadata))
    }

    async fn chain_properties(&mut self) -> SubeResult<crate::ChainProperties> {
        dispatch!(self, chain_properties())
    }

    async fn metadata(&mut self) -> SubeResult<Metadata> {
        dispatch!(self, metadata())
    }

    async fn block_info(&mut self, at: Option<u32>) -> SubeResult<crate::meta::BlockInfo> {
        dispatch!(self, block_info(at))
    }
}

// --- ChainSession implementation ---

impl crate::rpc::chainhead::ChainSession for AnyBackend {
    fn retain_block(&mut self, block_hash: &str) {
        match self {
            #[cfg(feature = "ws")]
            AnyBackend::Ws(backend) => backend.retain_block(block_hash),
            #[cfg(all(feature = "ws-web", target_arch = "wasm32"))]
            AnyBackend::WsWeb(backend) => backend.retain_block(block_hash),
            #[cfg(all(feature = "smoldot", feature = "std"))]
            AnyBackend::Smoldot(backend) => backend.retain_block(block_hash),
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    fn release_block(&mut self, block_hash: &str) {
        match self {
            #[cfg(feature = "ws")]
            AnyBackend::Ws(backend) => backend.release_block(block_hash),
            #[cfg(all(feature = "ws-web", target_arch = "wasm32"))]
            AnyBackend::WsWeb(backend) => backend.release_block(block_hash),
            #[cfg(all(feature = "smoldot", feature = "std"))]
            AnyBackend::Smoldot(backend) => backend.release_block(block_hash),
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    async fn next_chain_event(&mut self) -> SubeResult<crate::rpc::chainhead::ChainEvent> {
        dispatch!(self, next_chain_event())
    }

    fn try_next_chain_event(&mut self) -> Option<crate::rpc::chainhead::ChainEvent> {
        match self {
            #[cfg(feature = "ws")]
            AnyBackend::Ws(b) => b.try_next_chain_event(),
            #[cfg(all(feature = "ws-web", target_arch = "wasm32"))]
            AnyBackend::WsWeb(b) => b.try_next_chain_event(),
            #[cfg(all(feature = "smoldot", feature = "std"))]
            AnyBackend::Smoldot(b) => b.try_next_chain_event(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    async fn header(&mut self, block_hash: &str) -> SubeResult<crate::rpc::chainhead::BlockHeader> {
        dispatch!(self, header(block_hash))
    }

    async fn runtime_call_at(
        &mut self,
        block_hash: &str,
        function: &str,
        call_data: &str,
    ) -> SubeResult<Vec<u8>> {
        dispatch!(self, runtime_call_at(block_hash, function, call_data))
    }

    async fn scan_pallets(&mut self) -> SubeResult<Vec<String>> {
        dispatch!(self, scan_pallets())
    }

    async fn metadata_filtered(&mut self, pallets: &[&str]) -> SubeResult<Metadata> {
        dispatch!(self, metadata_filtered(pallets))
    }

    async fn get_storage_at_hash(
        &mut self,
        block_hash: &str,
        keys: Vec<crate::RawKey>,
    ) -> SubeResult<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
        dispatch!(self, get_storage_at_hash(block_hash, keys))
    }
}

pub(crate) async fn get_metadata(
    backend: &mut AnyBackend,
    preloaded: Option<Metadata>,
) -> SubeResult<Rc<Metadata>> {
    let meta = match preloaded {
        Some(m) => m,
        None => backend.metadata().await.map_err(|_| Error::BadMetadata)?,
    };
    Ok(Rc::new(meta))
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
            #[cfg(all(feature = "ws-web", target_arch = "wasm32", not(feature = "ws")))]
            "ws" | "wss" => {
                let ws = crate::rpc::ws_web::Backend::new(url.to_string().as_str())
                    .await
                    .map_err(|e| Error::Node(format!("connecting to {url}: {e}")))?;
                let chainhead = ChainHead::new(ws)
                    .await
                    .map_err(|e| Error::Node(format!("chain session for {url}: {e}")))?;
                Ok(AnyBackend::WsWeb(Box::new(chainhead)))
            }
            _ => Err(Error::BadInput),
        }
    })
    .await
}

#[cfg(all(feature = "smoldot", feature = "std"))]
pub(crate) async fn connect_light(chain_spec: &str, timeout: Duration) -> SubeResult<AnyBackend> {
    with_timeout(timeout, async {
        let backend = crate::rpc::smoldot::Backend::new_std(chain_spec)?;
        let chainhead = ChainHead::new(backend).await?;
        Ok(AnyBackend::Smoldot(Box::new(chainhead)))
    })
    .await
}

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
