use core::future::{Future, IntoFuture};
use core::pin::Pin;

use alloc::sync::Arc;

use crate::backend::{chain_string_to_url, connect, get_metadata, AnyBackend};
use crate::extrinsic::{EncodeCall, ExtrinsicBody};
use crate::prelude::*;
use crate::{JsonValue, Metadata, Response, Result as SubeResult, Signer};

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
}

impl<S> TxBuilder<(), S> {
    fn body<B>(self, body: B) -> TxBuilder<B, S> {
        TxBuilder {
            body,
            path: self.path,
            signer: self.signer,
            nonce: self.nonce,
            extensions: self.extensions,
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

    fn into_parts(self) -> (String, ExtrinsicBody<B>, S) {
        let body = ExtrinsicBody {
            nonce: self.nonce,
            body: self.body,
            extensions: self.extensions,
        };
        (self.path, body, self.signer)
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
            },
        }
    }

    /// Set the call body using scales text format (one-liner shorthand).
    pub fn body_text<'a>(self, text: &'a str) -> OneShotCall<crate::Text<'a>, ()> {
        self.body(crate::Text(text))
    }
}

/// One-liner query: `sube("wss://host/pallet/item/key").await?`
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
/// Owns the backend connection. Metadata is cached globally.
pub struct Sube {
    backend: AnyBackend,
    metadata: Arc<Metadata>,
}

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

    async fn connect_with_options(
        url: &str,
        preloaded: Option<Metadata>,
        timeout: core::time::Duration,
    ) -> SubeResult<Self> {
        let url = chain_string_to_url(url)?;
        let mut backend = connect(&url, timeout).await?;
        let metadata = get_metadata(&mut backend, &url, preloaded).await?;
        Ok(Sube { backend, metadata })
    }

    /// Connect via smoldot light client (no external node needed).
    ///
    /// ```rust,ignore
    /// let chain = Sube::connect_light(include_str!("polkadot.json")).await?;
    /// let r = chain.query("system/account/0x1234").await?;
    /// ```
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
        Ok(Sube { backend, metadata })
    }

    /// Connect a parachain via smoldot light client.
    ///
    /// Both the parachain and relay chain specs are required.
    #[cfg(all(feature = "smoldot", feature = "std"))]
    pub async fn connect_light_para(chain_spec: &str, relay_spec: &str) -> SubeResult<Self> {
        let mut backend =
            crate::backend::connect_light_para(chain_spec, relay_spec, crate::DEFAULT_TIMEOUT)
                .await?;
        let metadata =
            crate::backend::get_metadata_by_key(&mut backend, "light://parachain", None).await?;
        Ok(Sube { backend, metadata })
    }

    /// Query a storage path.
    pub async fn query(&mut self, path: &str) -> SubeResult<Response> {
        let path = path.trim_matches('/');
        match path {
            "_meta" | "_meta/registry" => Ok(Response::Meta(Arc::clone(&self.metadata))),
            _ => crate::query(&mut self.backend, &self.metadata, path, None).await,
        }
    }

    /// Query a storage path at a specific block number.
    pub async fn query_at(&mut self, path: &str, block: u32) -> SubeResult<Response> {
        crate::query(
            &mut self.backend,
            &self.metadata,
            path.trim_matches('/'),
            Some(block),
        )
        .await
    }

    /// Build an extrinsic call for the given pallet/method path.
    pub fn call(&mut self, path: &str) -> CallBuilder<'_, (), ()> {
        CallBuilder {
            backend: &mut self.backend,
            metadata: Arc::clone(&self.metadata),
            tx: TxBuilder {
                path: path.trim_matches('/').into(),
                body: (),
                signer: (),
                nonce: None,
                extensions: Vec::new(),
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
}

// --- CallBuilder (for reusable handle) ---

/// Builder for an extrinsic submission via a reusable [`Sube`] handle.
pub struct CallBuilder<'a, Body = (), Sign = ()> {
    backend: &'a mut AnyBackend,
    metadata: Arc<Metadata>,
    tx: TxBuilder<Body, Sign>,
}

impl<'a, S> CallBuilder<'a, (), S> {
    pub fn body<B>(self, body: B) -> CallBuilder<'a, B, S> {
        CallBuilder {
            backend: self.backend,
            metadata: self.metadata,
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
    pub fn body_text(self, text: &'a str) -> CallBuilder<'a, crate::Text<'a>, S> {
        self.body(crate::Text(text))
    }
}

impl<'a, B> CallBuilder<'a, B, ()> {
    pub fn signer<S>(self, signer: S) -> CallBuilder<'a, B, S> {
        CallBuilder {
            backend: self.backend,
            metadata: self.metadata,
            tx: self.tx.signer(signer),
        }
    }
}

impl<'a, B, S> CallBuilder<'a, B, S> {
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.tx = self.tx.nonce(nonce);
        self
    }

    pub fn with_extension(mut self, identifier: &str, value: JsonValue) -> Self {
        self.tx = self.tx.with_extension(identifier, value);
        self
    }
}

impl<'a, B, S> CallBuilder<'a, B, S>
where
    B: EncodeCall + core::fmt::Debug + 'a,
    S: Signer + 'a,
{
}

impl<'a, B, S> IntoFuture for CallBuilder<'a, B, S>
where
    B: EncodeCall + core::fmt::Debug + 'a,
    S: Signer + 'a,
{
    type Output = SubeResult<Response>;
    type IntoFuture = BoxFuture<'a, SubeResult<Response>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let (path, body, signer) = self.tx.into_parts();
            crate::extrinsic::submit(self.backend, &self.metadata, &path, body, signer).await
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
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.tx = self.tx.nonce(nonce);
        self
    }

    pub fn with_extension(mut self, identifier: &str, value: JsonValue) -> Self {
        self.tx = self.tx.with_extension(identifier, value);
        self
    }
}

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

            let (_, body, signer) = self.tx.into_parts();
            crate::extrinsic::submit(&mut backend, &meta, path, body, signer).await
        })
    }
}
