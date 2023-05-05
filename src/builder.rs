use crate::http::Backend as HttpBackend;
use crate::meta::Meta;
use crate::{prelude::*, NoSigner};
use crate::ws::{Backend as WSbackend, WS2};
use crate::{
    meta::BlockInfo, Backend, Error, ExtrinicBody, Metadata, Response, Result as SubeResult, SignerFn,
    StorageKey,
};

use async_trait::async_trait;
use core::future::{Future, IntoFuture};
use core::pin::Pin;
use heapless::Vec as HVec;
use once_cell::sync::OnceCell;
use scale_info::build;
use serde::Serializer;
use url::Url;



type PairHostBackend<'a> = (&'a str, AnyBackend, Metadata);

static INSTANCE: OnceCell<HVec<PairHostBackend, 10>> = OnceCell::new();


pub struct SubeBuilder<'a, Signer = NoSigner, Body = ()>
where
    Body: serde::Serialize
{
    url: Option<&'a str>,
    nonce: Option<u64>,
    body: Option<Body>,
    sender: Option<&'a [u8]>,
    signer: Option<Signer>,
    metadata: Option<Metadata>,
}

// default for non body queries
impl<'a, Signer, Body> Default for SubeBuilder<'a, Signer, Body> 
where 
    Body: serde::Serialize,
    Signer: SignerFn
{
    fn default() -> Self {
        SubeBuilder {
            url: None,
            nonce: None,
            body: None,
            sender: None,
            signer: Option::<NoSigner>::None,
            metadata: None,
        }
    }
}



impl<'a, Signer, Body> SubeBuilder<'a, Signer, Body>
where
    Body: serde::Serialize,
    Signer: SignerFn
{
       
    pub fn with_url(self, url: &'a str) -> Self {
        Self {
            url: Some(url),
            ..self
        }
    }

    pub fn with_body(self, body: Body) -> Self {
        Self {
            body: Some(body),
            ..self
        }
    }

    pub fn with_signer(self, signer: Signer) -> Self {
        Self {
            signer: Some(signer),
            ..self
        }
    }

    pub fn with_nonce(self, nonce: u64) -> Self {
        Self {
            nonce: Some(nonce),
            ..self
        }
    }

    pub fn with_meta(self, metadata: Metadata) -> Self {
        Self {
            metadata: Some(metadata),
            ..self
        }
    }

    pub fn with_sender(self, sender: &'a [u8]) -> Self {
        Self {
            sender: Some(sender),
            ..self
        }
    }
}

static BACKEND: async_once_cell::OnceCell<AnyBackend> = async_once_cell::OnceCell::new();
static META: async_once_cell::OnceCell<Metadata> = async_once_cell::OnceCell::new();




impl<'a, Signer, Body> IntoFuture for SubeBuilder<'a, Signer, Body>
where
    Body: serde::Serialize + core::fmt::Debug,
    Signer: SignerFn
{
    type Output = SubeResult<Response<'a>>;
    type IntoFuture = impl Future<Output = Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let Self {
            url,
            nonce,
            body,
            sender,
            signer,
            metadata,
        } = self;

        async move {
            println!("GETTING chain string to url");
            let url = chain_string_to_url(&url.ok_or(Error::BadInput)?)?;

            let backend = BACKEND
                .get_or_try_init(get_backend_by_url(url.clone()))
                .await?;

            println!("backend");
            let meta = META
                .get_or_try_init(async {
                    match metadata {
                        Some(m) => Ok(m),
                        None => backend.metadata().await.map_err(|_| Error::BadMetadata),
                    }
                })
                .await?;

            println!("meta");

            let path = url.path();

            println!("path {:?}", path);
            Ok(match path {
                "_meta" => Response::Meta(meta),
                "_meta/registry" => Response::Registry(&meta.types),
                _ => {
                    if let Some(tx_data) = body {
                        let signer = signer.ok_or(Error::BadInput)?;
                        crate::submit(backend, meta, path, ExtrinicBody {
                            nonce: nonce,
                            body: tx_data,
                            from: sender.ok_or(Error::BadInput)?
                        }, signer).await?
                    } else {
                        crate::query(&backend, meta, path).await?
                    }
                }
            })
        }
    }
}

fn chain_string_to_url(chain: &str) -> SubeResult<Url> {
    let chain = if !chain.starts_with("ws://")
        && !chain.starts_with("wss://")
        && !chain.starts_with("http://")
        && !chain.starts_with("https://")
    {
        ["wss", &chain].join("://")
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

async fn get_backend_by_url(url: Url) -> SubeResult<AnyBackend> {
    match url.scheme() {
        "ws" | "wss" => Ok(AnyBackend::Ws(
            WSbackend::new_ws2(url.to_string().as_str()).await?,
        )),
        "http" | "https" => Ok(AnyBackend::Http(HttpBackend::new(url))),
        _ => Err(Error::BadInput),
    }
}

enum AnyBackend {
    Http(HttpBackend),
    Ws(WSbackend<WS2>),
}

#[async_trait]
impl Backend for &AnyBackend {
    async fn metadata(&self) -> SubeResult<Metadata> {
        match self {
            AnyBackend::Http(b) => b.metadata().await,
            AnyBackend::Ws(b) => b.metadata().await,
        }
    }

    async fn submit<U: AsRef<[u8]> + Send>(&self, ext: U) -> SubeResult<()> {
        match self {
            AnyBackend::Http(b) => b.submit(ext).await,
            AnyBackend::Ws(b) => b.submit(ext).await,
        }
    }

    async fn block_info(&self, at: Option<u32>) -> SubeResult<BlockInfo> {
        match self {
            AnyBackend::Http(b) => b.block_info(at).await,
            AnyBackend::Ws(b) => b.block_info(at).await,
        }
    }
    async fn query_storage(&self, key: &StorageKey) -> SubeResult<Vec<u8>> {
        match self {
            AnyBackend::Http(b) => b.query_storage(key).await,
            AnyBackend::Ws(b) => b.query_storage(&key).await,
        }
    }
}


#[inline]
async fn get_metadata(b: &AnyBackend, metadata: Option<Metadata>) -> SubeResult<Metadata> {
    match metadata {
        Some(m) => Ok(m),
        None => Ok(b.metadata().await?)
    }
}

#[macro_export]
macro_rules! sube {

    ($url:expr) => {
        async {
            $crate::builder::SubeBuilder::default().with_url($url).await
        }
    };

    // Two parameters
    // Match when the macro is called with an expression (url) followed by a block of key-value pairs
    ( $url:literal => { $($key:ident: $value:expr),+ $(,)? }) => {
        
        async {
            let mut builder = $crate::builder::SubeBuilder::default().with_url($url);

            use paste::paste;
            
            paste!($(
                builder = builder.[<with_ $key>]($value);
            )*);
    
            // Return the updated builder
            builder.await
        }
    };
}
