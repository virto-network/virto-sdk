use crate::Result;
use async_std::path::PathBuf;
use std::str::FromStr;
use structopt::StructOpt;

/// SUBmit Extrinsics and query chain data
#[derive(StructOpt, Debug)]
#[structopt(name = "sube")]
pub(crate) struct Opt {
    /// Address of the chain to connect to. Http protocol assumed if not provided.
    ///
    /// When the metadata option is provided but not the chain, only offline functionality is
    /// supported
    #[structopt(short, long, default_value = "localhost")]
    pub chain: String,
    /// Format for the output (json,json-pretty,scale,hex)
    #[structopt(short, long, default_value = "json")]
    pub output: Output,
    /// Use existing metadata from the filesystem(in SCALE format)
    #[structopt(short, long)]
    pub metadata: Option<PathBuf>,
    #[structopt(short, long)]
    pub quiet: bool,
    #[structopt(short, long, parse(from_occurrences))]
    pub verbose: usize,

    #[structopt(value_name = "QUERY/CALL")]
    pub input: String,
}

#[derive(Debug)]
pub(crate) enum Output {
    Json(bool),
    Scale,
    Hex,
}

impl Output {
    pub(crate) fn format<O>(&self, out: O) -> Result<Vec<u8>>
    where
        O: serde::Serialize + Into<Vec<u8>>,
    {
        println!("Hello world {:?}", self);
        Ok(match self {
            Output::Json(pretty) => {
                if *pretty {
                    serde_json::to_vec_pretty(&out)?
                } else {
                    serde_json::to_vec(&out)?
                }
            }
            Output::Scale => out.into(),
            Output::Hex => format!("0x{}", hex::encode(out.into())).into(),
        })
    }
}

impl FromStr for Output {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "json" => Output::Json(false),
            "json-pretty" => Output::Json(true),
            "scale" => Output::Scale,
            "hex" => Output::Hex,
            _ => Output::Json(false),
        })
    }
}