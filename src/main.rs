use anyhow::Result;
use async_std::{io, prelude::*};
use codec::Encode;
use std::{convert::Infallible, str::FromStr};
use structopt::StructOpt;
use sube::{
    http::{Backend, Url},
    Backend as _, Sube,
};

#[derive(StructOpt, Debug)]
#[structopt(name = "sube")]
struct Opt {
    /// Node address
    #[structopt(short, long, default_value = "127.0.0.1")]
    pub chain: String,
    #[structopt(short = "p", long, default_value = "9933")]
    pub port: String,
    #[structopt(short, long, default_value = "Scale")]
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

    let node_url = Url::parse(&format!("http://{}:{}", opt.chain, opt.port))?;
    let s: Sube<_> = Backend::new(node_url).into();
    let meta = s.try_init_meta().await?;

    match opt.cmd {
        Cmd::Query { query } => {
            let res: String = s.query(query.as_str()).await?;
            writeln!(io::stdout(), "{}", res).await?;
        }
        Cmd::Submit => s.submit(io::stdin()).await?,
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
            _ => Output::Scale,
        })
    }
}
