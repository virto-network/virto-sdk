use frame_metadata::RuntimeMetadata;
use std::io::stdout;
use std::{convert::Infallible, io::Write, str::FromStr};
use structopt::StructOpt;
use sube::{http, Backend, Error, Sube};
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
}

#[async_std::main]
async fn main() {
    let opt = Opt::from_args();
    stderrlog::new()
        .verbosity(opt.verbose)
        .quiet(opt.quiet)
        .init()
        .unwrap();

    let node_url = Url::parse(&format!("http://{}:{}", opt.node, opt.node_port)).expect("Node URL");
    let s: Sube<_> = http::Backend::new(node_url).into();

    let meta = match s.metadata().await {
        Err(Error::Node(err)) => {
            log::error!("{}", err);
            return;
        }
        Err(err) => {
            log::error!("{:?}", err);
            return;
        }
        Ok(m) => m,
    };
    let meta: &'static RuntimeMetadata = Box::leak(meta.into());
    Sube::<http::Backend>::init_metadata(&meta);

    let response = match opt.cmd {
        Cmd::Query { query } => s.query(query.as_str()).await,
    };

    match response {
        Err(err) => log::error!("{:?}", err),
        Ok(res) => {
            let _ = writeln!(stdout().lock(), "{}", res);
        }
    };
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
