use anyhow::{anyhow, Result};
use async_std::{io, path::PathBuf, prelude::*};
use async_trait::async_trait;
use codec::{Decode, Encode};
use std::{convert::Infallible, str::FromStr};
use structopt::{clap::arg_enum, StructOpt};
use sube::{http, meta::*, ws, Backend, StorageKey, Sube};
use surf::Url;

/// SUBmit Extrinsics and query chain data
#[derive(StructOpt, Debug)]
#[structopt(name = "sube")]
struct Opt {
    /// Address of the chain to connect to. Http protocol assumed if not provided.
    ///
    /// When the metadata option is provided but not the chain, only offline functionality is
    /// supported
    #[structopt(short, long, required_unless = "metadata")]
    pub chain: Option<String>,
    /// Format for the output (json,scale,hex)
    #[structopt(short, long, default_value = "json")]
    pub output: Output,
    /// Use existing metadata from the filesystem(in SCALE format)
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
    /// Query the chain metadata
    #[structopt(visible_alias = "m")]
    Meta {
        #[structopt(subcommand)]
        cmd: Option<MetaOpt>,
    },
    /// Explore the type registry
    #[structopt(visible_alias = "r")]
    Registry {
        #[structopt(
            short = "t",
            possible_values = &RegOpt::variants(), 
            case_insensitive = true,
            requires = "query",
        )]
        query_type: Option<RegOpt>,
        query: Option<String>,
    },
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
    Decode {
        /// An id or the name of a type that exists in the type registry
        ty: String,
    },
}

#[derive(StructOpt, Debug)]
enum MetaOpt {
    /// Get the chain metadata (default)
    Get,
    /// Get information about pallets
    #[structopt(visible_alias = "p")]
    Pallets {
        #[structopt(long)]
        name_only: bool,
        #[structopt(short, long, conflicts_with = "constants", requires = "name")]
        entries: bool,
        #[structopt(short, long, requires = "name")]
        constants: bool,
        name: Option<String>,
    },
    /// Get information about the extrinsic format
    #[structopt(visible_alias = "e")]
    Extrinsic,
}

arg_enum! {
    #[derive(Debug)]
    enum RegOpt {
        Id,
        Name,
        Entry,
    }
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
    let mut opt = Opt::from_args();
    stderrlog::new()
        .verbosity(opt.verbose)
        .quiet(opt.quiet)
        .init()
        .unwrap();

    let client = get_client(&mut opt).await?;
    let meta = client.metadata().await?;
    let output = &opt.output;

    let out: Vec<_> = match opt.cmd {
        Cmd::Query { query } => output.format(client.query(&query).await?)?,
        Cmd::Submit => {
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input).await?;
            client.submit(input).await?;
            vec![]
        }
        Cmd::Meta { cmd } => match cmd {
            Some(MetaOpt::Pallets {
                name_only,
                name,
                entries,
                constants,
            }) => {
                if let Some(name) = name {
                    if let Some(p) = meta.pallet_by_name(&name) {
                        if name_only && !entries && !constants {
                            output.format(&p.name)?
                        } else if entries {
                            let entries = p
                                .storage()
                                .ok_or(anyhow!("No storage in pallet"))?
                                .entries();
                            if name_only {
                                output.format(entries.map(|e| &e.name).collect::<Vec<_>>())?
                            } else {
                                output.format(&entries.collect::<Vec<_>>())?
                            }
                        } else if constants {
                            if name_only {
                                output.format(
                                    &p.constants.iter().map(|c| &c.name).collect::<Vec<_>>(),
                                )?
                            } else {
                                output.format(&p.constants)?
                            }
                        } else {
                            output.format(p)?
                        }
                    } else {
                        return Err(anyhow!("No pallet named '{}'", name));
                    }
                } else if name_only {
                    let names = meta.pallets.iter().map(|p| &p.name).collect::<Vec<_>>();
                    output.format(&names)?
                } else {
                    output.format(&meta.pallets)?
                }
            }
            Some(MetaOpt::Extrinsic) => output.format(&meta.extrinsic)?,
            _ => output.format(meta)?,
        },
        Cmd::Registry { query_type, query } => {
            let reg = &meta.types;
            match (query_type, query) {
                (Some(RegOpt::Id), Some(q)) => {
                    let id = q.parse::<u32>()?;
                    let ty = reg.resolve(id).ok_or(anyhow!("Not in registry"))?;
                    output.format(ty)?
                }
                (Some(RegOpt::Name), Some(q)) => {
                    let ty = reg.find(&q);
                    if ty.is_empty() {
                        return Err(anyhow!("Not in registry"));
                    }
                    if ty.len() == 1 {
                        output.format(ty[0])?
                    } else {
                        output.format(ty)?
                    }
                }
                (Some(RegOpt::Entry), Some(q)) => {
                    let (pallet, item, _) =
                        StorageKey::parse_uri(&q).ok_or_else(|| anyhow!("Invalid entry format"))?;
                    let ty = meta
                        .storage_entry(&pallet, &item)
                        .ok_or_else(|| anyhow!("Not in registry"))?
                        .ty();
                    output.format(ty)?
                }
                _ => output.format(&meta.types)?,
            }
        }
        Cmd::Encode => {
            todo!()
        }
        Cmd::Decode { ty } => {
            let reg = client.registry().await?;
            let ty = ty
                .parse::<u32>()
                .ok()
                .or_else(|| reg.find_ids(&ty).first().copied())
                .ok_or_else(|| anyhow!("Not in registry"))?;
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input).await?;
            output.format(client.decode(input, ty).await?)?
        }
    };
    io::stdout().write_all(&out).await?;
    writeln!(io::stdout()).await?;
    Ok(())
}

async fn get_client(opt: &mut Opt) -> Result<Sube<AnyBackend>> {
    let url = chain_string_to_url(opt.chain.take())?;
    let mut maybe_meta = get_meta_from_fs(&opt.metadata).await;

    log::debug!("Matching backend for {}", url);
    let backend = match url.scheme() {
        "http" | "https" => AnyBackend::Http(http::Backend::new(url)),
        "ws" | "wss" => AnyBackend::Ws(ws::Backend::new_ws2(url.as_ref()).await?),
        "about" => AnyBackend::Offline(sube::Offline(
            maybe_meta
                .take()
                .ok_or(anyhow!("Couldn't get metadata from disk"))?,
        )),
        s => return Err(anyhow!("{} not supported", s)),
    };

    Ok(if let Some(meta) = maybe_meta {
        Sube::new_with_meta(backend, meta)
    } else {
        backend.into()
    })
}

async fn get_meta_from_fs(path: &Option<PathBuf>) -> Option<Metadata> {
    if path.is_none() {
        return None;
    }
    let mut m = Vec::new();
    let mut f = async_std::fs::File::open(path.as_ref().unwrap())
        .await
        .ok()?;
    f.read_to_end(&mut m).await.ok()?;
    Metadata::decode(&mut m.as_slice()).ok()
}

#[derive(Debug)]
enum Output {
    Json,
    Scale,
    Hex,
}

impl Output {
    fn format<O>(&self, out: O) -> Result<Vec<u8>>
    where
        O: serde::Serialize + Encode,
    {
        Ok(match self {
            Output::Json => serde_json::to_vec(&out)?,
            Output::Scale => out.encode(),
            Output::Hex => format!("0x{}", hex::encode(out.encode())).into(),
        })
    }
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
fn chain_string_to_url(chain: Option<String>) -> Result<Url> {
    if chain.is_none() {
        return Ok(Url::parse("about:offline")?);
    }
    let mut chain = chain.unwrap();
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
    Offline(sube::Offline),
}

#[async_trait]
impl Backend for AnyBackend {
    async fn query_storage(&self, key: &StorageKey) -> sube::Result<Vec<u8>> {
        match self {
            AnyBackend::Ws(b) => b.query_storage(key).await,
            AnyBackend::Http(b) => b.query_storage(key).await,
            AnyBackend::Offline(b) => b.query_storage(key).await,
        }
    }

    async fn submit<T>(&self, ext: T) -> sube::Result<()>
    where
        T: AsRef<[u8]> + Send,
    {
        match self {
            AnyBackend::Ws(b) => b.submit(ext).await,
            AnyBackend::Http(b) => b.submit(ext).await,
            AnyBackend::Offline(b) => b.submit(ext).await,
        }
    }

    async fn metadata(&self) -> sube::Result<sube::Metadata> {
        match self {
            AnyBackend::Ws(b) => b.metadata().await,
            AnyBackend::Http(b) => b.metadata().await,
            AnyBackend::Offline(b) => b.metadata().await,
        }
    }
}
