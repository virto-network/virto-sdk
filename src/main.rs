use anyhow::Result;
use async_std::{io, prelude::*};
use frame_metadata::RuntimeMetadata;
use std::{convert::Infallible, str::FromStr};
use structopt::StructOpt;
use sube::{http::Backend, Backend as _, Sube};
use url::Url;

#[derive(StructOpt, Debug)]
#[structopt(name = "sube")]
struct Opt {
    /// Node address
    #[structopt(short, long, default_value = "localhost")]
    pub node: String,
    #[structopt(short = "p", long, default_value = "9933")]
    pub node_port: String,
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
    Query { query: String },
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

    let node_url = Url::parse(&format!("http://{}:{}", opt.node, opt.node_port))?;
    let s: Sube<_> = Backend::new(node_url).into();

    let meta = s.metadata().await?;
    let meta: &'static RuntimeMetadata = Box::leak(meta.into());
    Sube::<Backend>::init_metadata(&meta);

    match opt.cmd {
        Cmd::Query { query } => {
            let res = s.query(query.as_str()).await?;
            writeln!(io::stdout(), "{}", res).await?;
        }
        Cmd::Submit => s.submit(io::stdin()).await?,
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
