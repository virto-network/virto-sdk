use anyhow::{anyhow, Result};
use async_std::{io, prelude::*};
use async_trait::async_trait;
use codec::Encode;
use std::{convert::Infallible, str::FromStr};
use structopt::StructOpt;
use sube::{http, ws, Backend, Sube};
use surf::Url;

#[derive(StructOpt, Debug)]
#[structopt(name = "sube")]
struct Opt {
    /// Node address
    #[structopt(short, long)]
    pub chain: String,
    #[structopt(short, long, default_value = "json")]
    pub output: Output,
    #[structopt(short, long)]
    pub quiet: bool,
    #[structopt(short, long, parse(from_occurrences))]
    pub verbose: usize,

    #[structopt(subcommand)]
    pub cmd: Cmd,
}

#[derive(StructOpt, Debug)]
enum Cmd {
    /// Get the chain metadata
    Meta,
    /// Use a path-like syntax to query data from the chain storage
    ///
    /// A storage item can be accessed as `module/item[/key[/key2]]`(e.g. `timestamp/now` or `system/account/0x123`).
    Query { query: String },
    /// Submit an extrinsic to the chain
    Submit,
}

#[async_std::main]
async fn main() {
    match run().await {
        Ok(_) => {}
        Err(err) => {
            log::error!("{}", err);
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<()> {
    let opt = Opt::from_args();
    stderrlog::new()
        .verbosity(opt.verbose)
        .quiet(opt.quiet)
        .init()
        .unwrap();

    let url = chain_string_to_url(opt.chain)?;

    log::debug!("Matching backend for {}", url);
    let backend: AnyBackend = match url.scheme() {
        "http" => AnyBackend::Http(http::Backend::new(url)),
        "ws" => AnyBackend::Ws(ws::Backend::new_ws2(url.as_ref()).await?),
        _ => return Err(anyhow!("Not supported")),
    };
    let client = Sube::from(backend);
    let meta = client.try_init_meta().await?;

    match opt.cmd {
        Cmd::Query { query } => {
            let res: String = client.query(query.as_str()).await?;
            writeln!(io::stdout(), "{}", res).await?;
        }
        Cmd::Submit => client.submit(io::stdin()).await?,
        Cmd::Meta => {
            let meta = match opt.output {
                Output::Scale => meta.encode(),
                Output::Json => serde_json::to_string(&meta)?.into(),
            };
            io::stdout().write_all(&meta).await?;
        }
    };
    Ok(())
}

#[derive(Debug)]
enum Output {
    Json,
    Scale,
}

impl FromStr for Output {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "json" => Output::Json,
            "scale" => Output::Scale,
            _ => Output::Json,
        })
    }
}

// Function that tries to be "smart" about what the user might want to actually connect to
fn chain_string_to_url(mut chain: String) -> Result<Url> {
    if !chain.starts_with("ws://") && !chain.starts_with("http://") {
        chain = ["http", &chain].join("://");
    }

    let mut url = Url::parse(&chain)?;
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

enum AnyBackend {
    Ws(ws::Backend<ws::WS2>),
    Http(http::Backend),
}

#[async_trait]
impl Backend for AnyBackend {
    async fn query_bytes<K>(&self, key: K) -> sube::Result<Vec<u8>>
    where
        K: std::convert::TryInto<sube::StorageKey, Error = sube::Error> + Send,
    {
        match self {
            AnyBackend::Ws(b) => b.query_bytes(key).await,
            AnyBackend::Http(b) => b.query_bytes(key).await,
        }
    }

    async fn submit<T>(&self, ext: T) -> sube::Result<()>
    where
        T: io::Read + Send + Unpin,
    {
        match self {
            AnyBackend::Ws(b) => b.submit(ext).await,
            AnyBackend::Http(b) => b.submit(ext).await,
        }
    }

    async fn metadata(&self) -> sube::Result<sube::Metadata> {
        match self {
            AnyBackend::Ws(b) => b.metadata().await,
            AnyBackend::Http(b) => b.metadata().await,
        }
    }
}
