use anyhow::{anyhow, Result};
use async_std::{io, path::PathBuf, prelude::*};
use async_trait::async_trait;
use codec::{Decode, Encode};
use std::{convert::Infallible, str::FromStr};
use structopt::StructOpt;
use sube::{http, ws, Backend, Metadata, StorageKey, Sube};
use surf::Url;

/// SUBmit Extrinsics and query chain data
#[derive(StructOpt, Debug)]
#[structopt(name = "sube")]
struct Opt {
    /// Address of the chain to connect to
    #[structopt(short, long)]
    pub chain: String,
    /// Format for the output (json,scale,hex)
    #[structopt(short, long, default_value = "json")]
    pub output: Output,
    /// Use an existing metadata definition from the filesystem
    #[structopt(short, long)]
    pub metadata: Option<PathBuf>,
    #[structopt(short, long)]
    pub quiet: bool,
    #[structopt(short, long, parse(from_occurrences))]
    pub verbose: usize,

    #[structopt(subcommand)]
    pub cmd: Cmd,
}

#[derive(StructOpt, Debug)]
enum Cmd {
    /// Get the chain metadata from the specified chain
    Meta,
    /// Use a path-like syntax to query data from the chain storage
    ///
    /// A storage item can be accessed as `pallet/item[/key[/key2]]`(e.g. `timestamp/now` or `system/account/0x123`).
    #[structopt(visible_alias = "q")]
    Query { query: String },
    /// Submit an extrinsic to the chain
    #[structopt(visible_alias = "s")]
    Submit,
    /// Convert human-readable data(JSON atm.) to SCALE format
    #[structopt(visible_alias = "e")]
    Encode,
    /// Convert SCALE encoded data to a human-readable format(JSON)
    #[structopt(visible_alias = "d")]
    Decode,
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
        "http" | "https" => AnyBackend::Http(http::Backend::new(url)),
        "ws" | "wss" => AnyBackend::Ws(ws::Backend::new_ws2(url.as_ref()).await?),
        _ => return Err(anyhow!("Not supported")),
    };

    let client: Sube<_> = if opt.metadata.is_none() {
        backend.into()
    } else {
        let meta_path = &opt.metadata.unwrap();
        let mut m = Vec::new();
        let mut f = async_std::fs::File::open(meta_path).await?;
        f.read_to_end(&mut m).await?;
        let meta = Metadata::decode(&mut m.as_slice())?;
        Sube::new_with_meta(backend, meta)
    };
    let meta = client.metadata().await?;

    let out: Vec<_> = match opt.cmd {
        Cmd::Query { query } => {
            let value = client.query(&query).await?;
            match opt.output {
                Output::Scale => value.as_ref().into(),
                Output::Json => value.to_string().as_bytes().into(),
                Output::Hex => format!("0x{}", hex::encode(value.as_ref()))
                    .as_bytes()
                    .into(),
            }
        }
        Cmd::Submit => {
            let mut input = String::new();
            io::stdin().read_line(&mut input).await?;
            client.submit(input).await?;
            vec![]
        }
        Cmd::Meta => match opt.output {
            Output::Scale => meta.encode(),
            Output::Json => serde_json::to_string(&meta)?.into(),
            Output::Hex => format!("0x{}", hex::encode(meta.encode())).into(),
        },
        Cmd::Encode => {
            todo!()
        }
        Cmd::Decode => {
            todo!()
        }
    };
    io::stdout().write_all(&out).await?;
    writeln!(io::stdout()).await?;
    Ok(())
}

#[derive(Debug)]
enum Output {
    Json,
    Scale,
    Hex,
}

impl FromStr for Output {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "json" => Output::Json,
            "scale" => Output::Scale,
            "hex" => Output::Hex,
            _ => Output::Json,
        })
    }
}

// Function that tries to be "smart" about what the user might want to actually connect to
fn chain_string_to_url(mut chain: String) -> Result<Url> {
    if !chain.starts_with("ws://")
        && !chain.starts_with("wss://")
        && !chain.starts_with("http://")
        && !chain.starts_with("https://")
    {
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
    async fn query_bytes(&self, key: &StorageKey) -> sube::Result<Vec<u8>> {
        match self {
            AnyBackend::Ws(b) => b.query_bytes(key).await,
            AnyBackend::Http(b) => b.query_bytes(key).await,
        }
    }

    async fn submit<T>(&self, ext: T) -> sube::Result<()>
    where
        T: AsRef<[u8]> + Send,
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
