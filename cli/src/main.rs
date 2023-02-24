use anyhow::{anyhow, Result};
use async_std::{
    io::{self, ReadExt, WriteExt},
    path::PathBuf,
    task::block_on,
};
use codec::Decode;
use opts::Opt;
use structopt::StructOpt;
use sube::{sube, Backend, Metadata};
use url::Url;

mod opts;

async fn run() -> Result<()> {
    let opt = Opt::from_args();
    stderrlog::new()
        .verbosity(opt.verbose)
        .quiet(opt.quiet)
        .init()
        .unwrap();

    let url = chain_string_to_url(&opt.chain)?;
    // let backend = sube::ws::Backend::new_ws2(url.as_str()).await?;
    let backend = sube::http::Backend::new(url.as_str());
    let meta = if let Some(m) = opt.metadata {
        get_meta_from_fs(&m)
            .await
            .ok_or_else(|| anyhow!("Couldn't read Metadata from file"))?
    } else {
        backend.metadata().await?
    };

    let res = sube(backend, &meta, &opt.input, None, |_, _| {}).await?;

    io::stdout().write_all(&opt.output.format(res)?).await?;
    writeln!(io::stdout()).await?;
    Ok(())
}

fn main() {
    block_on(async {
        match run().await {
            Ok(_) => {}
            Err(err) => {
                log::error!("{}", err);
                std::process::exit(1);
            }
        }
    })
}

// Function that tries to be "smart" about what the user might want to actually connect to
fn chain_string_to_url(chain: &str) -> Result<Url> {
    let chain = if !chain.starts_with("ws://")
        && !chain.starts_with("wss://")
        && !chain.starts_with("http://")
        && !chain.starts_with("https://")
    {
        ["wss", &chain].join("://")
    } else {
        chain.into()
    };

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

async fn get_meta_from_fs(path: &PathBuf) -> Option<Metadata> {
    let mut m = Vec::new();
    let mut f = async_std::fs::File::open(path).await.ok()?;
    f.read_to_end(&mut m).await.ok()?;
    Metadata::decode(&mut m.as_slice()).ok()
}