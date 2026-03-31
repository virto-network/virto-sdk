use core::future::{Future, IntoFuture};
use core::pin::Pin;

use alloc::sync::Arc;

use crate::extrinsic::{EncodeCall, ExtrinsicBody};
use crate::prelude::*;
use crate::{Backend, JsonValue, Metadata, Response, Result as SubeResult, Signer};

#[cfg(any(feature = "ws", feature = "smoldot"))]
use crate::backend::{chain_string_to_url, connect, get_metadata, AnyBackend};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

// --- TxBuilder: shared call configuration ---

/// Extrinsic call configuration — body, signer, nonce, extensions.
///
/// Shared by both [`CallBuilder`] (reusable handle) and [`OneShotCall`] (one-liner).
struct TxBuilder<Body, Sign> {
    path: String,
    body: Body,
    signer: Sign,
    nonce: Option<u64>,
    extensions: Vec<(String, JsonValue)>,
    wait_for_finalization: bool,
}

impl<S> TxBuilder<(), S> {
    fn body<B>(self, body: B) -> TxBuilder<B, S> {
        TxBuilder {
            body,
            path: self.path,
            signer: self.signer,
            nonce: self.nonce,
            extensions: self.extensions,
            wait_for_finalization: self.wait_for_finalization,
        }
    }
}

impl<B> TxBuilder<B, ()> {
    fn signer<S>(self, signer: S) -> TxBuilder<B, S> {
        TxBuilder {
            signer,
            path: self.path,
            body: self.body,
            nonce: self.nonce,
            extensions: self.extensions,
            wait_for_finalization: self.wait_for_finalization,
        }
    }
}

impl<B, S> TxBuilder<B, S> {
    fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self.extensions.retain(|(id, _)| id != "CheckNonce");
        self.extensions
            .push(("CheckNonce".into(), crate::json!(nonce)));
        self
    }

    fn with_extension(mut self, identifier: &str, value: JsonValue) -> Self {
        self.extensions.retain(|(id, _)| id != identifier);
        self.extensions.push((identifier.into(), value));
        self
    }

    fn into_parts(self) -> (String, ExtrinsicBody<B>, S, bool) {
        let body = ExtrinsicBody {
            nonce: self.nonce,
            body: self.body,
            extensions: self.extensions,
        };
        (self.path, body, self.signer, self.wait_for_finalization)
    }
}

// --- SubeBuilder: entry point ---

/// Lazy handle returned by [`sube()`](crate::sube).
///
/// ```rust,ignore
/// // One-liner query (URL includes path)
/// let r = sube("wss://kreivo.io/system/account/0x1234").await?;
///
/// // One-liner submit
/// sube("wss://kreivo.io/balances/transfer")
///     .body(json!({ "dest": {"Id": dest}, "value": 1000 }))
///     .signer(my_signer)
///     .await?;
///
/// // Reusable handle
/// let chain = Sube::connect("wss://kreivo.io").await?;
/// let r = chain.query("system/account/0x1234").await?;
/// ```
pub struct SubeBuilder {
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

    /// Set the extrinsic body (one-liner shorthand for submit).
    pub fn body<B>(self, body: B) -> OneShotCall<B, ()> {
        OneShotCall {
            url: self.url,
            preloaded_meta: self.metadata,
            timeout: self.timeout,
            tx: TxBuilder {
                path: String::new(),
                body,
                signer: (),
                nonce: None,
                extensions: Vec::new(),
                wait_for_finalization: false,
            },
        }
    }

    /// Set the call body using scales text format (one-liner shorthand).
    pub fn body_text<'a>(self, text: &'a str) -> OneShotCall<crate::Text<'a>, ()> {
        self.body(crate::Text(text))
    }
}

/// One-liner query: `sube("wss://host/pallet/item/key").await?`
#[cfg(any(feature = "ws", feature = "smoldot"))]
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
            let meta = get_metadata(&mut backend, &url, self.metadata).await?;

            Ok(match path {
                "/" | "" | "_meta" | "_meta/registry" => Response::Meta(Arc::clone(&meta)),
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
/// let mut chain = Sube::from_parts(chain_head, Arc::new(meta));
///
/// // Both support the same query API:
/// let r = chain.query("system/account/0x1234").await?;
/// ```
/// When `ws` or `smoldot` is enabled, `B` defaults to
/// [`AnyBackend`](crate::backend::AnyBackend) and includes URL/timeout
/// for reconnect support.
#[cfg(any(feature = "ws", feature = "smoldot"))]
pub struct Sube<B = AnyBackend> {
    backend: B,
    metadata: Arc<Metadata>,
    url: String,
    timeout: core::time::Duration,
}

#[cfg(not(any(feature = "ws", feature = "smoldot")))]
pub struct Sube<B> {
    backend: B,
    metadata: Arc<Metadata>,
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
    /// let mut chain = Sube::from_parts(chain_head, Arc::new(meta));
    /// ```
    pub fn from_parts(backend: B, metadata: Arc<Metadata>) -> Self {
        Sube {
            backend,
            metadata,
            #[cfg(any(feature = "ws", feature = "smoldot"))]
            url: String::new(),
            #[cfg(any(feature = "ws", feature = "smoldot"))]
            timeout: crate::DEFAULT_TIMEOUT,
        }
    }

    /// Mutable access to the metadata (for replacing it after construction).
    pub fn metadata_mut(&mut self) -> &mut Arc<Metadata> {
        &mut self.metadata
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
            "_meta" | "_meta/registry" => Ok(Response::Meta(Arc::clone(&self.metadata))),
            _ => crate::query(&mut self.backend, &self.metadata, path, None).await,
        }
    }

    /// Query a storage path at a specific block number.
    pub async fn query_at(&mut self, path: &str, block: u32) -> SubeResult<Response> {
        let path = path.trim_matches('/');
        crate::query(&mut self.backend, &self.metadata, path, Some(block)).await
    }

    /// Build an extrinsic call for the given pallet/method path.
    pub fn call(&mut self, path: &str) -> CallBuilder<'_, B, (), ()> {
        CallBuilder {
            sube: self,
            tx: TxBuilder {
                path: path.trim_matches('/').into(),
                body: (),
                signer: (),
                nonce: None,
                extensions: Vec::new(),
                wait_for_finalization: false,
            },
        }
    }

    /// Access the chain's metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Get a shared reference-counted handle to the metadata.
    pub fn metadata_arc(&self) -> Arc<Metadata> {
        Arc::clone(&self.metadata)
    }

    /// Access the type registry.
    pub fn registry(&self) -> &crate::Registry {
        &self.metadata.registry
    }

    /// Access the underlying backend.
    pub fn backend(&mut self) -> &mut B {
        &mut self.backend
    }
}

// --- ChainSession methods (chain events, block-pinned queries) ---

#[cfg(any(feature = "ws", feature = "ws-edge", feature = "smoldot"))]
impl<B: crate::rpc::chainhead::ChainSession> Sube<B> {
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
            "_meta" | "_meta/registry" => Ok(Response::Meta(Arc::clone(&self.metadata))),
            _ => {
                let (pallet, item_or_call, mut keys) =
                    crate::parse_uri(path).ok_or(crate::Error::BadInput)?;
                let pallet = self
                    .metadata
                    .pallet_by_name(&pallet)
                    .ok_or(crate::Error::PalletNotFound(pallet))?;

                if item_or_call == "_constants" {
                    let const_name = keys.pop().ok_or(crate::Error::MissingConstantName)?;
                    let const_meta = pallet
                        .constants
                        .iter()
                        .find(|c| c.name == const_name)
                        .ok_or(crate::Error::ConstantNotFound(const_name))?;
                    return Ok(Response::Value(
                        crate::StorageEntry::new(const_meta.value.clone(), const_meta.ty),
                        Arc::clone(&self.metadata),
                    ));
                }

                if let Ok(key_res) = crate::metadata::StorageKey::build_with_registry(
                    &self.metadata.registry,
                    pallet,
                    &item_or_call,
                    &keys,
                ) {
                    if !key_res.is_partial() {
                        let results = self
                            .backend
                            .get_storage_at_hash(block_hash, vec![key_res.key()])
                            .await?;
                        let value = results.into_iter().next().and_then(|(_, v)| v);
                        return Ok(match value {
                            None => Response::None,
                            Some(data) => Response::Value(
                                crate::StorageEntry::new(data, key_res.ty),
                                Arc::clone(&self.metadata),
                            ),
                        });
                    }
                }

                Err(crate::Error::BadInput)
            }
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
        self.metadata = Arc::new(meta);
        Ok(())
    }
}

// --- Edge backend constructor (embedded) ---

/// The concrete `Sube` type returned by [`connect_edge`].
#[cfg(feature = "ws-edge")]
pub type EdgeSube =
    Sube<crate::rpc::chainhead::ChainHead<crate::rpc::edge::Backend<crate::rpc::edge::EdgeSocket>>>;

/// Re-export for callers to create static buffers.
#[cfg(feature = "ws-edge")]
pub use crate::rpc::edge::EdgeBuffers;

/// Connect to a Substrate chain from an embedded device.
///
/// Handles everything: DNS → TCP → TLS → WebSocket → ChainHead → filtered
/// metadata. Returns a [`Sube`] handle ready for human-readable queries.
///
/// `bufs` provides reusable socket buffers — store in a `static`:
///
/// ```rust,ignore
/// static BUFS: EdgeBuffers = EdgeBuffers::new();
/// let rng = esp_hal::rng::Trng::try_new()?;
/// let mut chain = sube::connect_edge(
///     "wss://kreivo.io", stack, rng, &BUFS, &["CollatorSelection"],
/// ).await?;
/// ```
#[cfg(feature = "ws-edge")]
pub async fn connect_edge(
    url: &str,
    stack: embassy_net::Stack<'static>,
    rng: impl rand_core::CryptoRng + Send + 'static,
    bufs: &'static crate::rpc::edge::EdgeBuffers,
    pallets: &[&str],
) -> SubeResult<EdgeSube> {
    let ws = crate::rpc::edge::edge_connect(url, stack, rng, bufs).await?;
    log::info!("sube: starting ChainHead session");
    let mut chain_head = crate::rpc::chainhead::ChainHead::new(ws).await?;
    let meta = if pallets.is_empty() {
        log::info!("sube: ready (no metadata)");
        Arc::new(Metadata::empty())
    } else {
        log::info!("sube: loading filtered metadata (streaming)");
        let m = chain_head.metadata_filtered_streaming(pallets).await?;
        Arc::new(m)
    };
    Ok(Sube::from_parts(chain_head, meta))
}

// --- URL-based constructors and reconnect (AnyBackend only) ---

#[cfg(any(feature = "ws", feature = "smoldot"))]
use crate::rpc::chainhead::ChainSession as _;

#[cfg(any(feature = "ws", feature = "smoldot"))]
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
            metadata: Arc::new(metadata),
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
        let metadata = get_metadata(&mut backend, &url, preloaded).await?;
        Ok(Sube {
            backend,
            metadata,
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
        let metadata =
            crate::backend::get_metadata_by_key(&mut backend, "light://chain", preloaded).await?;
        Ok(Sube {
            backend,
            metadata,
            url: String::new(),
            timeout: crate::DEFAULT_TIMEOUT,
        })
    }

    /// Connect a parachain via smoldot light client.
    #[cfg(all(feature = "smoldot", feature = "std"))]
    pub async fn connect_light_para(chain_spec: &str, relay_spec: &str) -> SubeResult<Self> {
        let mut backend =
            crate::backend::connect_light_para(chain_spec, relay_spec, crate::DEFAULT_TIMEOUT)
                .await?;
        let metadata =
            crate::backend::get_metadata_by_key(&mut backend, "light://parachain", None).await?;
        Ok(Sube {
            backend,
            metadata,
            url: String::new(),
            timeout: crate::DEFAULT_TIMEOUT,
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
        Ok(())
    }
}

// --- CallBuilder (for reusable handle) ---

/// Builder for an extrinsic submission via a reusable [`Sube`] handle.
pub struct CallBuilder<'a, Bk: Backend, Body = (), Sign = ()> {
    sube: &'a mut Sube<Bk>,
    tx: TxBuilder<Body, Sign>,
}

impl<'a, Bk: Backend, S> CallBuilder<'a, Bk, (), S> {
    pub fn body<B>(self, body: B) -> CallBuilder<'a, Bk, B, S> {
        CallBuilder {
            sube: self.sube,
            tx: self.tx.body(body),
        }
    }

    /// Set the call body using scales text format.
    ///
    /// ```rust,ignore
    /// chain.call("balances/transfer_keep_alive")
    ///     .body_text("(dest:MultiAddress::Id(0xd435...);value:1000000)")
    ///     .signer(signer)
    ///     .await?;
    /// ```
    pub fn body_text(self, text: &'a str) -> CallBuilder<'a, Bk, crate::Text<'a>, S> {
        self.body(crate::Text(text))
    }
}

impl<'a, Bk: Backend, B> CallBuilder<'a, Bk, B, ()> {
    pub fn signer<S>(self, signer: S) -> CallBuilder<'a, Bk, B, S> {
        CallBuilder {
            sube: self.sube,
            tx: self.tx.signer(signer),
        }
    }
}

impl<'a, Bk: Backend, B, S> CallBuilder<'a, Bk, B, S> {
    /// Wait for full finalization instead of just best-chain inclusion.
    pub fn finalize(mut self) -> Self {
        self.tx.wait_for_finalization = true;
        self
    }

    pub fn nonce(mut self, nonce: u64) -> Self {
        self.tx = self.tx.nonce(nonce);
        self
    }

    pub fn with_extension(mut self, identifier: &str, value: JsonValue) -> Self {
        self.tx = self.tx.with_extension(identifier, value);
        self
    }
}

impl<'a, Bk: Backend, B, S> IntoFuture for CallBuilder<'a, Bk, B, S>
where
    B: EncodeCall + core::fmt::Debug + 'a,
    S: Signer + 'a,
{
    type Output = SubeResult<Response>;
    type IntoFuture = BoxFuture<'a, SubeResult<Response>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let (path, body, signer, finalize) = self.tx.into_parts();
            crate::extrinsic::submit(
                &mut self.sube.backend,
                &self.sube.metadata,
                &path,
                &body,
                &signer,
                finalize,
            )
            .await
        })
    }
}

// --- OneShotCall (for one-liner submits) ---

/// Builder for a one-liner extrinsic submit via [`sube()`](crate::sube).
pub struct OneShotCall<Body = (), Sign = ()> {
    url: String,
    preloaded_meta: Option<Metadata>,
    timeout: core::time::Duration,
    tx: TxBuilder<Body, Sign>,
}

impl<B> OneShotCall<B, ()> {
    pub fn signer<S>(self, signer: S) -> OneShotCall<B, S> {
        OneShotCall {
            url: self.url,
            preloaded_meta: self.preloaded_meta,
            timeout: self.timeout,
            tx: self.tx.signer(signer),
        }
    }
}

impl<B, S> OneShotCall<B, S> {
    /// Wait for full finalization instead of just best-chain inclusion.
    pub fn finalize(mut self) -> Self {
        self.tx.wait_for_finalization = true;
        self
    }

    pub fn nonce(mut self, nonce: u64) -> Self {
        self.tx = self.tx.nonce(nonce);
        self
    }

    pub fn with_extension(mut self, identifier: &str, value: JsonValue) -> Self {
        self.tx = self.tx.with_extension(identifier, value);
        self
    }
}

#[cfg(any(feature = "ws", feature = "smoldot"))]
impl<B, S> IntoFuture for OneShotCall<B, S>
where
    B: EncodeCall + core::fmt::Debug + 'static,
    S: Signer + 'static,
{
    type Output = SubeResult<Response>;
    type IntoFuture = BoxFuture<'static, SubeResult<Response>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let url = chain_string_to_url(&self.url)?;
            let path = url.path();
            let mut backend = connect(&url, self.timeout).await?;
            let meta = get_metadata(&mut backend, &url, self.preloaded_meta).await?;

            let (_, body, signer, finalize) = self.tx.into_parts();
            crate::extrinsic::submit(&mut backend, &meta, path, &body, &signer, finalize).await
        })
    }
}
