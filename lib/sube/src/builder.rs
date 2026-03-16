use core::future::IntoFuture;
use core::marker::PhantomData;

use crate::backend::{chain_string_to_url, get_multi_backend_by_url, BoxFuture};
use crate::extrinsic::ExtrinsicBody;
use crate::prelude::*;
use crate::{JsonValue, Metadata, Response, Result as SubeResult, Signer};

/// A reusable handle to a Substrate chain.
///
/// Created via [`sube()`](crate::sube). Can be used as a one-liner by awaiting
/// directly, or reused for multiple queries/calls via [`.query()`] and [`.call()`].
///
/// ```rust,ignore
/// // One-liner query
/// let result = sube("wss://kreivo.io/system/account/0x1234").await?;
///
/// // Reusable handle
/// let chain = sube("wss://kreivo.io");
/// let acct = chain.query("system/account/0x1234").await?;
/// let ver  = chain.query("system/_constants/Version").await?;
///
/// // Submit via handle
/// chain.call("balances/transfer")
///     .body(json!({ "dest": {"Id": dest}, "value": 1000 }))
///     .signer(my_signer)
///     .await?;
///
/// // One-liner submit
/// sube("wss://kreivo.io/balances/transfer")
///     .body(json!({ "dest": {"Id": dest}, "value": 1000 }))
///     .signer(my_signer)
///     .await?;
/// ```
pub struct Sube {
    url: String,
    metadata: Option<Metadata>,
}

impl Sube {
    pub(crate) fn new(url: &str) -> Self {
        Sube {
            url: url.into(),
            metadata: None,
        }
    }

    /// Provide pre-loaded metadata instead of fetching from the chain.
    pub fn with_meta(mut self, meta: Metadata) -> Self {
        self.metadata = Some(meta);
        self
    }

    /// Build a query for the given storage path.
    pub fn query(&self, path: &str) -> QueryBuilder {
        QueryBuilder {
            url: join_url(&self.url, path),
            metadata: self.metadata.clone(),
        }
    }

    /// Build an extrinsic call for the given pallet/method path.
    pub fn call<'a>(&self, path: &str) -> CallBuilder<'a, (), ()> {
        CallBuilder {
            url: join_url(&self.url, path),
            body: (),
            signer: (),
            nonce: None,
            metadata: self.metadata.clone(),
            extensions: Vec::new(),
            _lt: PhantomData,
        }
    }

    /// Set the extrinsic body (one-liner shorthand for submit).
    ///
    /// Uses the full URL passed to [`sube()`](crate::sube) as the call path.
    pub fn body<'a, B>(self, body: B) -> CallBuilder<'a, B, ()> {
        CallBuilder {
            url: self.url,
            body,
            signer: (),
            nonce: None,
            metadata: self.metadata,
            extensions: Vec::new(),
            _lt: PhantomData,
        }
    }
}

/// One-liner query: `sube("wss://host/pallet/item/key").await?`
impl IntoFuture for Sube {
    type Output = SubeResult<Response<'static>>;
    type IntoFuture = BoxFuture<'static, SubeResult<Response<'static>>>;

    fn into_future(self) -> Self::IntoFuture {
        let qb = QueryBuilder {
            url: self.url,
            metadata: self.metadata,
        };
        qb.into_future()
    }
}

fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

// --- QueryBuilder ---

/// Builder for a storage query. Implements `IntoFuture` — just `.await` it.
pub struct QueryBuilder {
    url: String,
    metadata: Option<Metadata>,
}

impl IntoFuture for QueryBuilder {
    type Output = SubeResult<Response<'static>>;
    type IntoFuture = BoxFuture<'static, SubeResult<Response<'static>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let url = chain_string_to_url(&self.url)?;

            let block = url
                .query_pairs()
                .find(|(k, _)| k == "at")
                .map(|(_, v)| v.parse::<u32>().expect("at query param must be a number"));

            let path = url.path();
            let (backend, meta) =
                get_multi_backend_by_url(url.clone(), self.metadata).await?;

            Ok(match path {
                "_meta" => Response::Meta(meta),
                "_meta/registry" => Response::Registry(&meta.registry),
                _ => crate::query(&backend, meta, path, block).await?,
            })
        })
    }
}

// --- CallBuilder ---

/// Builder for an extrinsic submission.
///
/// Chain `.body()`, `.signer()`, and optionally `.nonce()` / `.with_extension()`,
/// then `.await` to submit.
pub struct CallBuilder<'a, Body = (), Sign = ()> {
    url: String,
    body: Body,
    signer: Sign,
    nonce: Option<u64>,
    metadata: Option<Metadata>,
    extensions: Vec<(String, JsonValue)>,
    _lt: PhantomData<&'a ()>,
}

impl<'a, S> CallBuilder<'a, (), S> {
    /// Set the extrinsic call body.
    pub fn body<B>(self, body: B) -> CallBuilder<'a, B, S> {
        CallBuilder {
            url: self.url,
            body,
            signer: self.signer,
            nonce: self.nonce,
            metadata: self.metadata,
            extensions: self.extensions,
            _lt: PhantomData,
        }
    }
}

impl<'a, B> CallBuilder<'a, B, ()> {
    /// Set the signer for this extrinsic.
    pub fn signer<S>(self, signer: S) -> CallBuilder<'a, B, S> {
        CallBuilder {
            url: self.url,
            body: self.body,
            signer,
            nonce: self.nonce,
            metadata: self.metadata,
            extensions: self.extensions,
            _lt: PhantomData,
        }
    }
}

impl<'a, B, S> CallBuilder<'a, B, S> {
    /// Override the account nonce (also sets a `CheckNonce` extension override).
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self.extensions.retain(|(id, _)| id != "CheckNonce");
        self.extensions
            .push(("CheckNonce".into(), crate::json!(nonce)));
        self
    }

    /// Provide a value for a specific signed extension by identifier.
    pub fn with_extension(mut self, identifier: &str, value: JsonValue) -> Self {
        self.extensions.retain(|(id, _)| id != identifier);
        self.extensions.push((identifier.into(), value));
        self
    }
}

impl<'a, B, S> IntoFuture for CallBuilder<'a, B, S>
where
    B: serde::Serialize + core::fmt::Debug + 'a,
    S: Signer + 'a,
{
    type Output = SubeResult<Response<'static>>;
    type IntoFuture = BoxFuture<'a, SubeResult<Response<'static>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let url = chain_string_to_url(&self.url)?;
            let path = url.path();
            let (backend, meta) =
                get_multi_backend_by_url(url.clone(), self.metadata).await?;

            crate::extrinsic::submit(
                backend,
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
