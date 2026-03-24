use anyhow::Result;
use clap::Parser;

mod tui;

/// Sube — query and explore Substrate chains
#[derive(Parser)]
#[command(name = "sube", version)]
struct Cli {
    /// URL path to query, e.g. system/account/0x1234
    /// Defaults to kreivo (wss://kreivo.io)
    path: Option<String>,

    /// Chain endpoint (wss://kreivo.io by default)
    #[arg(short, long, default_value = "wss://kreivo.io")]
    chain: String,

    /// Output format: text (default) or json
    #[arg(short, long, default_value = "text")]
    format: String,

    /// Watch for changes (re-query on each new finalized block)
    #[arg(short, long)]
    watch: bool,
}

fn main() -> Result<()> {
    smol::block_on(run())
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.path {
        Some(path) if cli.watch => watch(&cli.chain, &path, &cli.format).await,
        Some(path) => oneshot(&cli.chain, &path, &cli.format).await,
        None => tui::run(&cli.chain).await,
    }
}

fn print_response(response: &sube::Response, format: &str) -> Result<()> {
    use sube::Response;
    match format {
        "json" => {
            if let Some(json) = response.to_json()? {
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("(none)");
            }
        }
        _ => match response {
            Response::None => println!("(none)"),
            Response::Value(entry, meta) => {
                println!("{}", entry.to_text(&meta.registry)?);
            }
            Response::ValueSet(items, meta) => {
                for (keys, value) in items {
                    let key_strs: Vec<String> = keys
                        .iter()
                        .filter_map(|k| k.to_text(&meta.registry).ok())
                        .collect();
                    let key_display = key_strs.join(", ");
                    match value {
                        Some(v) => println!("[{key_display}] {}", v.to_text(&meta.registry)?),
                        None => println!("[{key_display}] (none)"),
                    }
                }
            }
            Response::Meta(meta) => {
                for p in &meta.pallets {
                    println!("{}", p.name);
                }
            }
            Response::Void => {}
        },
    }
    Ok(())
}

async fn oneshot(chain: &str, path: &str, format: &str) -> Result<()> {
    eprintln!("Connecting to {chain}...");
    let mut chain = sube::Sube::connect(chain).await?;
    let response = chain.query(path).await?;
    print_response(&response, format)
}

async fn watch(chain_url: &str, path: &str, format: &str) -> Result<()> {
    eprintln!("Connecting to {chain_url}...");
    let mut chain = sube::Sube::connect(chain_url).await?;

    // Print initial value
    let response = chain.query(path).await?;
    print_response(&response, format)?;
    let mut prev_raw: Vec<u8> = match &response {
        sube::Response::Value(entry, _) => entry.data.clone(),
        _ => vec![],
    };

    loop {
        // Wait for a new block and query at that specific block
        let hash = loop {
            match chain.next_event().await? {
                sube::ChainEvent::NewBlock { hash, .. } => break hash,
                _ => continue,
            }
        };

        let response = chain.query_at_hash(path, &hash).await?;
        let current_raw = match &response {
            sube::Response::Value(entry, _) => entry.data.clone(),
            _ => vec![],
        };

        if current_raw != prev_raw {
            print_response(&response, format)?;
            prev_raw = current_raw;
        }
    }
}
