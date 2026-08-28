#[cfg(any(feature = "ws", feature = "smoldot-std"))]
use core::future::{Future, IntoFuture};
#[cfg(any(feature = "ws", feature = "smoldot-std"))]
use core::pin::Pin;

use alloc::rc::Rc;

use crate::extrinsic::{
    EncodeCall, EncodedExtrinsic, ExternalSigningRequest, PreparedCall, TransactionOptions,
    TransactionReceipt, TransactionReport, WaitFor,
};
use crate::prelude::*;
use crate::{
    Backend, ExtrinsicAssembler, Metadata, Response, Result as SubeResult, SignatureScheme,
    StoragePage,
};

#[cfg(any(feature = "ws", feature = "smoldot-std"))]
use crate::backend::{AnyBackend, chain_string_to_url, connect, get_metadata};

#[cfg(any(feature = "ws", feature = "smoldot-std"))]
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

// --- Call configuration ---

struct CallData<Body> {
    path: String,
    body: Body,
}

impl CallData<()> {
    fn body<B>(self, body: B) -> CallData<B> {
        CallData {
            body,
            path: self.path,
        }
    }
}

// --- SubeBuilder: entry point ---

/// Lazy handle returned by [`sube()`](crate::sube).
///
/// ```rust,ignore
/// // One-liner query (URL includes path)
/// let r = sube("wss://kreivo.io/system/account/0x1234").await?;
///
/// // Reusable handle
/// let chain = Sube::connect("wss://kreivo.io").await?;
/// let r = chain.query("system/account/0x1234").await?;
/// ```
pub struct SubeBuilder {
    #[allow(dead_code)]
    url: String,
    metadata: Option<Metadata>,
    timeout: core::time::Duration,
}

impl SubeBuilder {
    pub(crate) fn new(url: &str) -> Self {
        SubeBuilder {
            url: url.into(),
            metadata: None,
            timeout: crate::DEFAULT_TIMEOUT,
        }
    }

    /// Provide pre-loaded metadata instead of fetching from the chain.
    pub fn with_meta(mut self, meta: Metadata) -> Self {
        self.metadata = Some(meta);
        self
    }

    /// Set the connection timeout (default: 30s).
    pub fn with_timeout(mut self, timeout: core::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// One-liner query: `sube("wss://host/pallet/item/key").await?`
#[cfg(any(feature = "ws", feature = "smoldot-std"))]
impl IntoFuture for SubeBuilder {
    type Output = SubeResult<Response>;
    type IntoFuture = BoxFuture<'static, SubeResult<Response>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let url = chain_string_to_url(&self.url)?;

            let block = url
                .query_pairs()
                .find(|(k, _)| *k == "at")
                .and_then(|(_, v)| v.parse::<u32>().ok());

            let path = url.path();
            let mut backend = connect(&url, self.timeout).await?;
            let meta = get_metadata(&mut backend, self.metadata).await?;

            Ok(match path {
                "/" | "" | "_meta" | "_meta/registry" => Response::Meta(Rc::clone(&meta)),
                _ => crate::query(&mut backend, &meta, path, block).await?,
            })
        })
    }
}

// --- Sube (connected, reusable handle) ---

/// A connected handle to a Substrate chain.
///
/// Generic over the backend `B`. Use [`Sube::connect`] for URL-based connections
/// (returns `Sube<AnyBackend>`) or [`Sube::from_parts`] for pre-built backends
/// like `ChainHead<edge::Backend<T>>` on embedded targets.
///
/// ```rust,ignore
/// // From URL (std, returns Sube<AnyBackend>)
/// let mut chain = Sube::connect("wss://kreivo.io").await?;
///
/// // From pre-built backend (embedded, returns Sube<ChainHead<R>>)
/// let chain_head = ChainHead::new(ws).await?;
/// let meta = chain_head.metadata_filtered(&["CollatorSelection"]).await?;
/// let mut chain = Sube::from_parts(chain_head, Rc::new(meta));
///
/// // Both support the same query API:
/// let r = chain.query("system/account/0x1234").await?;
/// ```
/// When `ws` or `smoldot-std` is enabled, `B` defaults to
/// [`AnyBackend`] and includes URL/timeout
/// for reconnect support.
#[cfg(any(feature = "ws", feature = "smoldot-std"))]
pub struct Sube<B = AnyBackend> {
    backend: B,
    metadata: Rc<Metadata>,
    chain_properties: Option<crate::ChainProperties>,
    url: String,
    timeout: core::time::Duration,
}

#[cfg(not(any(feature = "ws", feature = "smoldot-std")))]
pub struct Sube<B> {
    backend: B,
    metadata: Rc<Metadata>,
    chain_properties: Option<crate::ChainProperties>,
}

// --- Generic methods (any Backend) ---

impl<B: Backend> Sube<B> {
    /// Wrap a pre-built backend with its metadata.
    ///
    /// This is the constructor for embedded targets where you bring your
    /// own transport (e.g. `ChainHead<edge::Backend<TlsSession>>`).
    ///
    /// ```rust,ignore
    /// let ws = edge::Backend::connect(session, "kreivo.io", "/").await?;
    /// let mut chain_head = ChainHead::new(ws).await?;
    /// let meta = chain_head.metadata_filtered(&["CollatorSelection"]).await?;
    /// let mut chain = Sube::from_parts(chain_head, Rc::new(meta));
    /// ```
    pub fn from_parts(backend: B, metadata: Rc<Metadata>) -> Self {
        Sube {
            backend,
            metadata,
            chain_properties: None,
            #[cfg(any(feature = "ws", feature = "smoldot-std"))]
            url: String::new(),
            #[cfg(any(feature = "ws", feature = "smoldot-std"))]
            timeout: crate::DEFAULT_TIMEOUT,
        }
    }

    /// Replace the metadata used for subsequent queries and submissions.
    pub fn set_metadata(&mut self, metadata: Metadata) {
        self.metadata = Rc::new(metadata);
    }

    /// Query a storage path using human-readable names.
    ///
    /// Path format: `pallet/storage_item/key1/key2/...`
    /// (kebab-case is converted to CamelCase automatically)
    ///
    /// ```rust,ignore
    /// let r = chain.query("system/account/0x1234").await?;
    /// let r = chain.query("collator-selection/last-authored-block").await?;
    /// ```
    pub async fn query(&mut self, path: &str) -> SubeResult<Response> {
        let path = path.trim_matches('/');
        match path {
            "_meta" | "_meta/registry" => Ok(Response::Meta(Rc::clone(&self.metadata))),
            _ => crate::query(&mut self.backend, &self.metadata, path, None).await,
        }
    }

    /// Query a storage path at a specific block number.
    pub async fn query_at(&mut self, path: &str, block: u32) -> SubeResult<Response> {
        let path = path.trim_matches('/');
        crate::query(&mut self.backend, &self.metadata, path, Some(block)).await
    }

    /// Query a constant or fully-keyed storage item at a known finalized hash.
    pub async fn query_at_finalized_hash(
        &mut self,
        path: &str,
        block_hash: [u8; 32],
    ) -> SubeResult<Response> {
        crate::query_at_hash(
            &mut self.backend,
            &self.metadata,
            path.trim_matches('/'),
            block_hash,
        )
        .await
    }

    /// Query a bounded page of a partially-keyed map at one finalized snapshot.
    ///
    /// Feed `StoragePage::next_key` back as `start_key` and the returned
    /// `StoragePage::at.number` back as `block` to continue the same scan.
    pub async fn query_page(
        &mut self,
        path: &str,
        limit: u16,
        start_key: Option<crate::RawKey>,
        block: Option<u32>,
    ) -> SubeResult<StoragePage> {
        crate::query_page(
            &mut self.backend,
            &self.metadata,
            path.trim_matches('/'),
            limit,
            start_key,
            block,
        )
        .await
    }

    /// Query a bounded map page at a known finalized hash. Feed
    /// `StoragePage::next_key` and the same `at` value into the next call to
    /// continue the exact snapshot without requiring an archive node.
    pub async fn query_page_at_hash(
        &mut self,
        path: &str,
        limit: u16,
        start_key: Option<crate::RawKey>,
        at: crate::BlockInfo,
    ) -> SubeResult<StoragePage> {
        crate::query_page_at_hash(
            &mut self.backend,
            &self.metadata,
            path.trim_matches('/'),
            limit,
            start_key,
            at,
        )
        .await
    }

    /// Build an extrinsic call for the given pallet/method path.
    pub fn call(&mut self, path: &str) -> CallBuilder<'_, B, ()> {
        CallBuilder {
            sube: self,
            call: CallData {
                path: path.trim_matches('/').into(),
                body: (),
            },
        }
    }

    /// Access the chain's metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Get a shared reference-counted handle to the metadata.
    pub fn metadata_rc(&self) -> Rc<Metadata> {
        Rc::clone(&self.metadata)
    }

    /// Access the type registry.
    pub fn registry(&self) -> &crate::Registry {
        &self.metadata.registry
    }

    /// Fetch chain properties once and return the cached value thereafter.
    pub async fn chain_properties(&mut self) -> SubeResult<&crate::ChainProperties> {
        if self.chain_properties.is_none() {
            self.chain_properties = Some(self.backend.chain_properties().await?);
        }
        Ok(self
            .chain_properties
            .as_ref()
            .expect("chain properties were initialized"))
    }

    /// Return chain properties when they have already been fetched.
    pub fn cached_chain_properties(&self) -> Option<&crate::ChainProperties> {
        self.chain_properties.as_ref()
    }

    /// Access the underlying backend.
    pub fn backend(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Metadata-validate and encode a runtime call. This operation is local:
    /// it does not sign, access the backend, or submit anything.
    pub fn prepare_call<V: EncodeCall + ?Sized>(
        &self,
        path: &str,
        body: &V,
    ) -> SubeResult<PreparedCall> {
        crate::extrinsic::prepare_call(&self.metadata, path.trim_matches('/'), body)
    }

    /// Sign and fully encode a prepared call. This may read chain context and
    /// the nonce, but never submits the transaction.
    pub async fn build_transaction(
        &mut self,
        call: &PreparedCall,
        assembler: &(impl ExtrinsicAssembler + ?Sized),
        options: TransactionOptions,
    ) -> SubeResult<EncodedExtrinsic> {
        crate::extrinsic::build_transaction(
            &mut self.backend,
            &self.metadata,
            call,
            &options,
            assembler,
        )
        .await
    }

    /// Freeze a V4 signing payload for an external wallet, hardware device, or
    /// separate actor. `nonce_account` may differ from `signing_account`.
    pub async fn prepare_external_signing(
        &mut self,
        call: &PreparedCall,
        signing_account: &[u8],
        nonce_account: &[u8],
        scheme: SignatureScheme,
        options: TransactionOptions,
    ) -> SubeResult<ExternalSigningRequest> {
        crate::extrinsic::prepare_external_signing(
            &mut self.backend,
            &self.metadata,
            call,
            signing_account,
            nonce_account,
            scheme,
            &options,
        )
        .await
    }

    /// Finish an external V4 signing request locally without submitting it.
    pub fn finish_external_signing(
        &self,
        request: &ExternalSigningRequest,
        signature: &[u8],
    ) -> SubeResult<EncodedExtrinsic> {
        crate::extrinsic::finish_external_signing(&self.metadata, request, signature)
    }

    /// Obtain fee, weight, and validity diagnostics without submitting.
    pub async fn inspect_transaction(
        &mut self,
        extrinsic: &EncodedExtrinsic,
    ) -> SubeResult<TransactionReport> {
        self.backend.inspect_transaction(extrinsic).await
    }

    /// Submit bytes that were explicitly built and reviewed.
    pub async fn submit_transaction(
        &mut self,
        extrinsic: &EncodedExtrinsic,
        wait_for: WaitFor,
    ) -> SubeResult<TransactionReceipt> {
        // Never silently re-sign stale bytes after a runtime upgrade.
        let live_metadata = self.backend.metadata().await?;
        let current = crate::extrinsic::runtime_versions(&live_metadata)?;
        if current != (extrinsic.spec_version, extrinsic.transaction_version) {
            return Err(crate::Error::RuntimeUpgrade {
                built_spec: extrinsic.spec_version,
                current_spec: current.0,
            });
        }
        let genesis = self.backend.block_info(Some(0)).await?.hash;
        if genesis != extrinsic.genesis_hash {
            return Err(crate::Error::GenesisMismatch);
        }
        let receipt = self.backend.submit_transaction(extrinsic, wait_for).await?;
        self.backend.enrich_receipt(receipt, &self.metadata).await
    }
}

// --- ChainSession methods (chain events, block-pinned queries) ---

#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
impl<B: crate::rpc::chainhead::ChainSession> Sube<B> {
    /// Retain a finalized block for a bounded multi-request snapshot scan.
    pub fn retain_finalized_hash(&mut self, block_hash: [u8; 32]) {
        self.backend
            .retain_block(&format!("0x{}", hex::encode(block_hash)));
    }

    /// Release a block previously retained with [`retain_finalized_hash`].
    pub fn release_finalized_hash(&mut self, block_hash: [u8; 32]) {
        self.backend
            .release_block(&format!("0x{}", hex::encode(block_hash)));
    }

    /// Wait for the next chain event (new block, finalization, best block change).
    ///
    /// ```rust,ignore
    /// loop {
    ///     match chain.next_event().await? {
    ///         ChainEvent::NewBlock { hash, parent, .. } => { /* new block */ }
    ///         ChainEvent::Finalized { hashes, .. } => { /* finalized */ }
    ///         ChainEvent::BestBlock { hash } => { /* head changed */ }
    ///     }
    /// }
    /// ```
    pub async fn next_event(&mut self) -> SubeResult<crate::rpc::chainhead::ChainEvent> {
        self.backend.next_chain_event().await
    }

    /// Return a buffered chain event without blocking, if any.
    pub fn try_next_event(&mut self) -> Option<crate::rpc::chainhead::ChainEvent> {
        self.backend.try_next_chain_event()
    }

    /// Wait for the next finalization event.
    ///
    /// Convenience method that skips `NewBlock` and `BestBlock` events,
    /// returning only when blocks are finalized.
    pub async fn next_finalized(&mut self) -> SubeResult<crate::rpc::chainhead::ChainEvent> {
        loop {
            let event = self.next_event().await?;
            if matches!(event, crate::rpc::chainhead::ChainEvent::Finalized { .. }) {
                return Ok(event);
            }
        }
    }

    /// Fetch the block header at a pinned block hash.
    pub async fn header(
        &mut self,
        block_hash: &str,
    ) -> SubeResult<crate::rpc::chainhead::BlockHeader> {
        self.backend.header(block_hash).await
    }

    /// Execute a runtime API call at a specific pinned block hash.
    pub async fn runtime_call_at(
        &mut self,
        block_hash: &str,
        function: &str,
        call_data: &str,
    ) -> SubeResult<Vec<u8>> {
        self.backend
            .runtime_call_at(block_hash, function, call_data)
            .await
    }

    /// Query storage at a specific block hash from a `NewBlock` event.
    ///
    /// The block hash must be from a recent `ChainEvent::NewBlock` that hasn't
    /// been finalized or pruned yet (it's pinned by the follow subscription).
    ///
    /// ```rust,ignore
    /// loop {
    ///     match chain.next_event().await? {
    ///         ChainEvent::NewBlock { hash, .. } => {
    ///             let r = chain.query_at_hash("system/account/0x1234", &hash).await?;
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    pub async fn query_at_hash(&mut self, path: &str, block_hash: &str) -> SubeResult<Response> {
        let path = path.trim_matches('/');
        match path {
            "_meta" | "_meta/registry" => Ok(Response::Meta(Rc::clone(&self.metadata))),
            _ => match crate::resolve_query(&self.metadata, path)? {
                crate::ResolvedQuery::Constant(entry) => {
                    Ok(Response::Value(entry, Rc::clone(&self.metadata)))
                }
                crate::ResolvedQuery::Storage(key) if key.is_partial() => {
                    Err(crate::Error::BadInput)
                }
                crate::ResolvedQuery::Storage(key) => {
                    let results = self
                        .backend
                        .get_storage_at_hash(block_hash, vec![key.key()])
                        .await?;
                    let value = results.into_iter().next().and_then(|(_, value)| value);
                    Ok(crate::storage_response(value, key.ty, &self.metadata))
                }
            },
        }
    }

    /// Scan available pallet names from the chain (lightweight, no type decode).
    pub async fn scan_pallets(&mut self) -> crate::Result<Vec<String>> {
        self.backend.scan_pallets().await
    }

    /// Reload metadata keeping only the specified pallets.
    ///
    /// Makes a fresh metadata request and decodes only types referenced
    /// by the selected pallets. Reduces memory from ~400KB to ~170KB
    /// for a typical 2-pallet selection.
    pub async fn load_filtered_metadata(&mut self, pallets: &[&str]) -> crate::Result<()> {
        let refs: Vec<&str> = pallets.to_vec();
        let meta = self.backend.metadata_filtered(&refs).await?;
        self.metadata = Rc::new(meta);
        Ok(())
    }
}

// --- Edge backend constructor (embedded) ---

/// The concrete `Sube` type returned by [`connect_edge`].
#[cfg(feature = "ws-edge")]
pub type EdgeSube =
    Sube<crate::rpc::chainhead::ChainHead<crate::rpc::edge::Backend<crate::rpc::edge::EdgeSocket>>>;

/// Re-export for callers.
#[cfg(feature = "ws-edge")]
pub use crate::rpc::edge::EdgeResources;

/// Connect to a Substrate chain from an embedded device.
///
/// Handles everything: DNS → TCP → TLS → WebSocket → ChainHead → filtered
/// metadata. Returns a [`Sube`] handle ready for human-readable queries.
///
/// ```rust,ignore
/// static RX: StaticCell<[u8; 2048]> = StaticCell::new();
/// static TX: StaticCell<[u8; 2048]> = StaticCell::new();
/// let resources = sube::EdgeResources::new(
///     stack,
///     RX.init([0; 2048]),
///     TX.init([0; 2048]),
/// )
/// .with_ca_certificate_der(include_bytes!("root-ca.der"));
/// let rng = esp_hal::rng::Trng::try_new()?;
/// let mut chain = sube::connect_edge(
///     "wss://kreivo.io",
///     resources,
///     rng,
///     &["CollatorSelection"],
/// )
///     .await?;
/// ```
#[cfg(feature = "ws-edge")]
pub async fn connect_edge(
    url: &str,
    resources: crate::rpc::edge::EdgeResources,
    rng: impl rand_core::CryptoRng + Send + 'static,
    pallets: &[&str],
) -> SubeResult<EdgeSube> {
    let mut chain_head = connect_edge_chain_head(url, resources, rng).await?;
    let metadata = chain_head.metadata_filtered(pallets).await?;
    log::info!("sube: ready");
    Ok(Sube::from_parts(chain_head, Rc::new(metadata)))
}

/// Connect from an embedded device using previously decoded metadata.
///
/// This avoids downloading and decoding metadata during connection, which is
/// useful when a device persists filtered metadata in flash.
#[cfg(feature = "ws-edge")]
pub async fn connect_edge_with_meta(
    url: &str,
    resources: crate::rpc::edge::EdgeResources,
    rng: impl rand_core::CryptoRng + Send + 'static,
    metadata: Metadata,
) -> SubeResult<EdgeSube> {
    let chain_head = connect_edge_chain_head(url, resources, rng).await?;
    log::info!("sube: ready with preloaded metadata");
    Ok(Sube::from_parts(chain_head, Rc::new(metadata)))
}

#[cfg(feature = "ws-edge")]
async fn connect_edge_chain_head(
    url: &str,
    resources: crate::rpc::edge::EdgeResources,
    rng: impl rand_core::CryptoRng + Send + 'static,
) -> SubeResult<
    crate::rpc::chainhead::ChainHead<crate::rpc::edge::Backend<crate::rpc::edge::EdgeSocket>>,
> {
    let ws = crate::rpc::edge::edge_connect(url, resources, rng).await?;
    log::info!("sube: starting ChainHead session");
    crate::rpc::chainhead::ChainHead::new(ws).await
}

// --- URL-based constructors and reconnect (AnyBackend only) ---

#[cfg(any(feature = "ws", feature = "smoldot-std"))]
use crate::rpc::chainhead::ChainSession as _;

#[cfg(any(feature = "ws", feature = "smoldot-std"))]
impl Sube {
    /// Connect to a chain and return a reusable handle.
    pub async fn connect(url: &str) -> SubeResult<Self> {
        Self::connect_with_options(url, None, crate::DEFAULT_TIMEOUT).await
    }

    /// Connect with a custom timeout.
    pub async fn connect_with_timeout(
        url: &str,
        timeout: core::time::Duration,
    ) -> SubeResult<Self> {
        Self::connect_with_options(url, None, timeout).await
    }

    /// Connect with pre-loaded metadata.
    pub async fn connect_with_meta(url: &str, preloaded: Option<Metadata>) -> SubeResult<Self> {
        Self::connect_with_options(url, preloaded, crate::DEFAULT_TIMEOUT).await
    }

    /// Connect with filtered metadata — only decode types for the specified pallets.
    ///
    /// This is the low-memory path: peak is ~200KB for 2 pallets vs ~1.5MB
    /// for a full connect. System pallet is always included.
    ///
    /// ```rust,ignore
    /// let mut chain = Sube::connect_filtered("wss://kreivo.io", &["Balances"]).await?;
    /// ```
    pub async fn connect_filtered(url: &str, pallets: &[&str]) -> SubeResult<Self> {
        let parsed = chain_string_to_url(url)?;
        let mut backend = connect(&parsed, crate::DEFAULT_TIMEOUT).await?;
        let metadata = backend.metadata_filtered(pallets).await?;
        Ok(Sube {
            backend,
            metadata: Rc::new(metadata),
            chain_properties: None,
            url: url.into(),
            timeout: crate::DEFAULT_TIMEOUT,
        })
    }

    async fn connect_with_options(
        url_str: &str,
        preloaded: Option<Metadata>,
        timeout: core::time::Duration,
    ) -> SubeResult<Self> {
        let url = chain_string_to_url(url_str)?;
        let mut backend = connect(&url, timeout).await?;
        let metadata = get_metadata(&mut backend, preloaded).await?;
        Ok(Sube {
            backend,
            metadata,
            chain_properties: None,
            url: url_str.into(),
            timeout,
        })
    }

    /// Connect via smoldot light client (no external node needed).
    #[cfg(all(feature = "smoldot", feature = "std"))]
    pub async fn connect_light(chain_spec: &str) -> SubeResult<Self> {
        Self::connect_light_with_meta(chain_spec, None).await
    }

    /// Connect light client with pre-loaded metadata.
    #[cfg(all(feature = "smoldot", feature = "std"))]
    pub async fn connect_light_with_meta(
        chain_spec: &str,
        preloaded: Option<Metadata>,
    ) -> SubeResult<Self> {
        let mut backend = crate::backend::connect_light(chain_spec, crate::DEFAULT_TIMEOUT).await?;
        let metadata = get_metadata(&mut backend, preloaded).await?;
        Ok(Sube {
            backend,
            metadata,
            chain_properties: None,
            url: String::new(),
            timeout: crate::DEFAULT_TIMEOUT,
        })
    }

    /// Connect a parachain via smoldot light client.
    #[cfg(all(feature = "smoldot", feature = "std"))]
    pub async fn connect_light_para(chain_spec: &str, relay_spec: &str) -> SubeResult<Self> {
        Self::connect_light_para_with_timeout(chain_spec, relay_spec, crate::DEFAULT_TIMEOUT).await
    }

    /// Connect a parachain via smoldot with an explicit initialization
    /// timeout. Initial relay warp sync and parachain runtime proofs can take
    /// longer than the general-purpose default on a cold light client.
    #[cfg(all(feature = "smoldot", feature = "std"))]
    pub async fn connect_light_para_with_timeout(
        chain_spec: &str,
        relay_spec: &str,
        timeout: core::time::Duration,
    ) -> SubeResult<Self> {
        let mut backend =
            crate::backend::connect_light_para(chain_spec, relay_spec, timeout).await?;
        let metadata = get_metadata(&mut backend, None).await?;
        Ok(Sube {
            backend,
            metadata,
            chain_properties: None,
            url: String::new(),
            timeout,
        })
    }

    /// Re-establish the backend connection using the stored URL.
    pub async fn reconnect(&mut self) -> SubeResult<()> {
        if self.url.is_empty() {
            return Err(crate::Error::ChainUnavailable);
        }
        log::info!("reconnecting to {}", self.url);
        let url = chain_string_to_url(&self.url)?;
        self.backend = connect(&url, self.timeout).await?;
        self.chain_properties = None;
        Ok(())
    }
}

// --- CallBuilder (for reusable handle) ---

/// Builder for an extrinsic submission via a reusable [`Sube`] handle.
pub struct CallBuilder<'a, Bk: Backend, Body = ()> {
    sube: &'a mut Sube<Bk>,
    call: CallData<Body>,
}

impl<'a, Bk: Backend> CallBuilder<'a, Bk, ()> {
    pub fn body<B>(self, body: B) -> CallBuilder<'a, Bk, B> {
        CallBuilder {
            sube: self.sube,
            call: self.call.body(body),
        }
    }

    /// Set the call body using scales text format.
    ///
    /// ```rust,ignore
    /// let call = chain.call("balances/transfer_keep_alive")
    ///     .body_text("(dest:MultiAddress::Id(0xd435...);value:1000000)")
    ///     .prepare()?;
    /// ```
    pub fn body_text(self, text: &'a str) -> CallBuilder<'a, Bk, crate::Text<'a>> {
        self.body(crate::Text(text))
    }
}

impl<'a, Bk: Backend, B> CallBuilder<'a, Bk, B>
where
    B: EncodeCall,
{
    /// Prepare the call without signing or submitting it.
    pub fn prepare(self) -> SubeResult<PreparedCall> {
        self.sube.prepare_call(&self.call.path, &self.call.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    struct MockBackend {
        submissions: Rc<Cell<usize>>,
        property_reads: Rc<Cell<usize>>,
        metadata: Metadata,
    }

    impl Backend for MockBackend {
        async fn get_storage_items(
            &mut self,
            _keys: Vec<crate::RawKey>,
            _block: Option<u32>,
        ) -> SubeResult<Vec<(crate::RawKey, Option<crate::RawValue>)>> {
            Ok(Vec::new())
        }

        async fn get_keys_paged(
            &mut self,
            _from: crate::RawKey,
            _size: u16,
            _to: Option<crate::RawKey>,
        ) -> SubeResult<Vec<crate::RawKey>> {
            Ok(Vec::new())
        }

        async fn submit(&mut self, _ext: &[u8], _wait_for_finalization: bool) -> SubeResult<()> {
            self.submissions.set(self.submissions.get() + 1);
            Ok(())
        }

        async fn metadata(&mut self) -> SubeResult<Metadata> {
            Ok(self.metadata.clone())
        }

        async fn chain_properties(&mut self) -> SubeResult<crate::ChainProperties> {
            self.property_reads.set(self.property_reads.get() + 1);
            Ok(crate::ChainProperties {
                ss58_format: Some(2),
                token_symbols: vec!["UNIT".into()],
                token_decimals: vec![12],
            })
        }

        async fn block_info(&mut self, at: Option<u32>) -> SubeResult<crate::metadata::BlockInfo> {
            let number = if at == Some(0) { 0 } else { 128 };
            Ok(crate::metadata::BlockInfo {
                number,
                hash: if number == 0 { [1; 32] } else { [2; 32] },
                parent: if number == 0 { [1; 32] } else { [3; 32] },
            })
        }
    }

    #[test]
    fn building_and_inspecting_never_submit() {
        smol::block_on(async {
            let metadata =
                Metadata::from_bytes(include_bytes!("../tests/fixtures/kreivo.scale")).unwrap();
            let submissions = Rc::new(Cell::new(0));
            let property_reads = Rc::new(Cell::new(0));
            let backend = MockBackend {
                submissions: Rc::clone(&submissions),
                property_reads: Rc::clone(&property_reads),
                metadata: metadata.clone(),
            };
            let mut chain = Sube::from_parts(backend, Rc::new(metadata));
            let signer = crate::SignerFn::new([7; 32], |_payload: &[u8]| async { Ok([9; 64]) });

            let call = chain
                .prepare_call("system/remark", &crate::Text("(remark:0x0102)"))
                .unwrap();
            let encoded = chain
                .build_transaction(&call, &signer, TransactionOptions::default().nonce(0))
                .await
                .unwrap();
            assert_eq!(submissions.get(), 0);
            assert_eq!(encoded.checkpoint_number, 128);
            assert_eq!(encoded.expires_at, Some(192));

            let report = chain.inspect_transaction(&encoded).await.unwrap();
            assert_eq!(submissions.get(), 0);
            assert!(report.partial_fee.is_none());

            assert_eq!(chain.chain_properties().await.unwrap().ss58_format, Some(2));
            assert_eq!(chain.chain_properties().await.unwrap().ss58_format, Some(2));
            assert_eq!(property_reads.get(), 1);

            chain
                .submit_transaction(&encoded, WaitFor::BestBlock)
                .await
                .unwrap();
            assert_eq!(submissions.get(), 1);
        });
    }

    #[test]
    fn external_v4_signing_matches_the_signer_path_byte_for_byte() {
        smol::block_on(async {
            let metadata =
                Metadata::from_bytes(include_bytes!("../tests/fixtures/kreivo.scale")).unwrap();
            let backend = MockBackend {
                submissions: Rc::new(Cell::new(0)),
                property_reads: Rc::new(Cell::new(0)),
                metadata: metadata.clone(),
            };
            let mut chain = Sube::from_parts(backend, Rc::new(metadata));
            let account = [7; 32];
            let signature = [9; 64];
            let signer =
                crate::SignerFn::new(account, move |_payload: &[u8]| async move { Ok(signature) });
            let call = chain
                .prepare_call("system/remark", &crate::Text("(remark:0x0102)"))
                .unwrap();
            let options = TransactionOptions::default().nonce(5);

            let ordinary = chain
                .build_transaction(&call, &signer, options.clone())
                .await
                .unwrap();
            let request = chain
                .prepare_external_signing(
                    &call,
                    &account,
                    &account,
                    crate::SignatureScheme::Sr25519,
                    options,
                )
                .await
                .unwrap();
            let external = chain.finish_external_signing(&request, &signature).unwrap();

            assert_eq!(external.bytes, ordinary.bytes);
            assert_eq!(external.extensions, ordinary.extensions);
            assert_eq!(external.authorization, ordinary.authorization);
            assert_eq!(request.context.account_nonce, 5);
        });
    }

    #[test]
    fn external_signing_hashes_large_payloads_and_separates_identities() {
        smol::block_on(async {
            let metadata =
                Metadata::from_bytes(include_bytes!("../tests/fixtures/kreivo.scale")).unwrap();
            let backend = MockBackend {
                submissions: Rc::new(Cell::new(0)),
                property_reads: Rc::new(Cell::new(0)),
                metadata: metadata.clone(),
            };
            let mut chain = Sube::from_parts(backend, Rc::new(metadata));
            let body = format!("(remark:0x{})", hex::encode(vec![0x55; 300]));
            let call = chain
                .prepare_call("system/remark", &crate::Text(&body))
                .unwrap();
            let signing_account = [3; 32];
            let nonce_account = [4; 32];
            let request = chain
                .prepare_external_signing(
                    &call,
                    &signing_account,
                    &nonce_account,
                    crate::SignatureScheme::Ed25519,
                    TransactionOptions::default().nonce(11),
                )
                .await
                .unwrap();

            assert_eq!(request.signing_payload.len(), 32);
            assert_eq!(request.signing_account, signing_account);
            assert_eq!(request.nonce_account, nonce_account);
            let encoded = chain.finish_external_signing(&request, &[8; 64]).unwrap();
            assert_eq!(encoded.authorization.signing_account, signing_account);
            assert_eq!(encoded.authorization.nonce_account, nonce_account);
            assert_eq!(encoded.authorization.scheme.as_deref(), Some("Ed25519"));
            assert!(matches!(
                chain.finish_external_signing(&request, &[8; 63]),
                Err(crate::Error::Signing(_))
            ));
        });
    }
}
