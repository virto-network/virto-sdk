use std::{convert::Infallible, str::FromStr};

use structopt::StructOpt;
use sube::{http, Backend, Sube};
use url::Url;

#[derive(StructOpt, Debug)]
#[structopt(name = "demo")]
struct Opt {
    #[structopt(
        short = "c",
        long = "chain-url",
        default_value = "http://localhost:9933"
    )]
    pub chain_url: Url,
    #[structopt(short = "o", long = "output", default_value = "Scale")]
    pub output: Output,
}

#[async_std::main]
async fn main() {
    let opt = Opt::from_args();
    let s: Sube<_> = http::Backend::new(opt.chain_url).into();
    s.query("module/foo").await;
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
