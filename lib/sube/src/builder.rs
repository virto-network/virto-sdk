use core::future::{Future, IntoFuture};
use core::marker::PhantomData;
use core::pin::Pin;

use crate::backend::{chain_string_to_url, connect, get_metadata, AnyBackend};
use crate::extrinsic::{EncodeCall, ExtrinsicBody};
use crate::prelude::*;
use crate::{JsonValue, Metadata, Response, Result as SubeResult, Signer};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Lazy handle returned by [`sube()`](crate::sube).
///
/// Can be used in two ways:
///
/// ```rust,ignore
/// // One-liner: connect + query (URL includes path)
/// let r = sube("wss://kreivo.io/system/account/0x1234").await?;
///
/// // One-liner: connect + submit
/// sube("wss://kreivo.io/balances/transfer")
///     .body(json!({ "dest": {"Id": dest}, "value": 1000 }))
///     .signer(my_signer)
///     .await?;
///
/// // Reusable handle (keeps connection alive)
/// let chain = Sube::connect("wss://kreivo.io").await?;
/// let r = chain.query("system/account/0x1234").await?;
/// chain.call("balances/transfer")
///     .body(json!({...}))
///     .signer(signer)
///     .await?;
/// ```
pub struct SubeBuilder {
    url: String,
    metadata: Option<Metadata>,
}

impl SubeBuilder {
    pub(crate) fn new(url: &str) -> Self {
        SubeBuilder {
            url: url.into(),
            metadata: None,
        }
    }

    /// Provide pre-loaded metadata instead of fetching from the chain.
    pub fn with_meta(mut self, meta: Metadata) -> Self {
        self.metadata = Some(meta);
        self
    }

    /// Set the extrinsic body (one-liner shorthand for submit).
    pub fn body<'a, B>(self, body: B) -> OneShotCall<'a, B, ()> {
        OneShotCall {
            url: self.url,
            preloaded_meta: self.metadata,
            body,
            signer: (),
            nonce: None,
            extensions: Vec::new(),
            _lt: PhantomData,
        }
    }

    /// Set the call body using scales text format (one-liner shorthand).
    pub fn body_text<'a>(self, text: &'a str) -> OneShotCall<'a, crate::Text<'a>, ()> {
        self.body(crate::Text(text))
    }
}

/// One-liner query: `sube("wss://host/pallet/item/key").await?`
impl IntoFuture for SubeBuilder {
    type Output = SubeResult<Response<'static>>;
    type IntoFuture = BoxFuture<'static, SubeResult<Response<'static>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let url = chain_string_to_url(&self.url)?;

            let block = url
                .query_pairs()
                .find(|(k, _)| *k == "at")
                .map(|(_, v)| v.parse::<u32>().expect("at query param must be a number"));

            let path = url.path();
            let backend = connect(&url).await?;
            let meta = get_metadata(&backend, &url, self.metadata).await?;

            Ok(match path {
                "/" | "" => Response::Meta(meta),
                "_meta" => Response::Meta(meta),
                "_meta/registry" => Response::Registry(&meta.registry),
                _ => crate::query(&backend, meta, path, block).await?,
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
    metadata: &'static Metadata,
}

impl Sube {
    /// Connect to a chain and return a reusable handle.
    pub async fn connect(url: &str) -> SubeResult<Self> {
        Self::connect_with_meta(url, None).await
    }

    /// Connect with pre-loaded metadata.
    pub async fn connect_with_meta(url: &str, preloaded: Option<Metadata>) -> SubeResult<Self> {
        let url = chain_string_to_url(url)?;
        let backend = connect(&url).await?;
        let metadata = get_metadata(&backend, &url, preloaded).await?;
        Ok(Sube { backend, metadata })
    }

    /// Query a storage path.
    pub async fn query(&self, path: &str) -> SubeResult<Response<'static>> {
        let path = path.trim_matches('/');
        match path {
            "_meta" => Ok(Response::Meta(self.metadata)),
            "_meta/registry" => Ok(Response::Registry(&self.metadata.registry)),
            _ => crate::query(&self.backend, self.metadata, path, None).await,
        }
    }

    /// Query a storage path at a specific block number.
    pub async fn query_at(&self, path: &str, block: u32) -> SubeResult<Response<'static>> {
        crate::query(
            &self.backend,
            self.metadata,
            path.trim_matches('/'),
            Some(block),
        )
        .await
    }

    /// Build an extrinsic call for the given pallet/method path.
    pub fn call<'a>(&'a self, path: &str) -> CallBuilder<'a, (), ()> {
        CallBuilder {
            backend: &self.backend,
            metadata: self.metadata,
            path: path.trim_matches('/').into(),
            body: (),
            signer: (),
            nonce: None,
            extensions: Vec::new(),
            _lt: PhantomData,
        }
    }

    /// Access the chain's metadata.
    pub fn metadata(&self) -> &Metadata {
        self.metadata
    }

    /// Access the type registry.
    pub fn registry(&self) -> &crate::Registry {
        &self.metadata.registry
    }
}

// --- CallBuilder (for reusable handle) ---

/// Builder for an extrinsic submission via a reusable [`Sube`] handle.
pub struct CallBuilder<'a, Body = (), Sign = ()> {
    backend: &'a AnyBackend,
    metadata: &'static Metadata,
    path: String,
    body: Body,
    signer: Sign,
    nonce: Option<u64>,
    extensions: Vec<(String, JsonValue)>,
    _lt: PhantomData<&'a ()>,
}

impl<'a, S> CallBuilder<'a, (), S> {
    pub fn body<B>(self, body: B) -> CallBuilder<'a, B, S> {
        CallBuilder {
            backend: self.backend,
            metadata: self.metadata,
            path: self.path,
            body,
            signer: self.signer,
            nonce: self.nonce,
            extensions: self.extensions,
            _lt: PhantomData,
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
            path: self.path,
            body: self.body,
            signer,
            nonce: self.nonce,
            extensions: self.extensions,
            _lt: PhantomData,
        }
    }
}

impl<'a, B, S> CallBuilder<'a, B, S> {
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self.extensions.retain(|(id, _)| id != "CheckNonce");
        self.extensions
            .push(("CheckNonce".into(), crate::json!(nonce)));
        self
    }

    pub fn with_extension(mut self, identifier: &str, value: JsonValue) -> Self {
        self.extensions.retain(|(id, _)| id != identifier);
        self.extensions.push((identifier.into(), value));
        self
    }
}

impl<'a, B, S> IntoFuture for CallBuilder<'a, B, S>
where
    B: EncodeCall + core::fmt::Debug + 'a,
    S: Signer + 'a,
{
    type Output = SubeResult<Response<'static>>;
    type IntoFuture = BoxFuture<'a, SubeResult<Response<'static>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            crate::extrinsic::submit(
                self.backend,
                self.metadata,
                &self.path,
                ExtrinsicBody {
                    nonce: self.nonce,
                    body: self.body,
                    extensions: self.extensions,
                },
                self.signer,
            )
            .await
        })
    }
}

// --- OneShotCall (for one-liner submits) ---

/// Builder for a one-liner extrinsic submit via [`sube()`](crate::sube).
pub struct OneShotCall<'a, Body = (), Sign = ()> {
    url: String,
    preloaded_meta: Option<Metadata>,
    body: Body,
    signer: Sign,
    nonce: Option<u64>,
    extensions: Vec<(String, JsonValue)>,
    _lt: PhantomData<&'a ()>,
}

impl<'a, B> OneShotCall<'a, B, ()> {
    pub fn signer<S>(self, signer: S) -> OneShotCall<'a, B, S> {
        OneShotCall {
            url: self.url,
            preloaded_meta: self.preloaded_meta,
            body: self.body,
            signer,
            nonce: self.nonce,
            extensions: self.extensions,
            _lt: PhantomData,
        }
    }
}

impl<'a, B, S> OneShotCall<'a, B, S> {
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self.extensions.retain(|(id, _)| id != "CheckNonce");
        self.extensions
            .push(("CheckNonce".into(), crate::json!(nonce)));
        self
    }

    pub fn with_extension(mut self, identifier: &str, value: JsonValue) -> Self {
        self.extensions.retain(|(id, _)| id != identifier);
        self.extensions.push((identifier.into(), value));
        self
    }
}

impl<'a, B, S> IntoFuture for OneShotCall<'a, B, S>
where
    B: EncodeCall + core::fmt::Debug + 'a,
    S: Signer + 'a,
{
    type Output = SubeResult<Response<'static>>;
    type IntoFuture = BoxFuture<'a, SubeResult<Response<'static>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let url = chain_string_to_url(&self.url)?;
            let path = url.path();
            let backend = connect(&url).await?;
            let meta = get_metadata(&backend, &url, self.preloaded_meta).await?;

            crate::extrinsic::submit(
                &backend,
                meta,
                path,
                ExtrinsicBody {
                    nonce: self.nonce,
                    body: self.body,
                    extensions: self.extensions,
                },
                self.signer,
            )
            .await
        })
    }
}
