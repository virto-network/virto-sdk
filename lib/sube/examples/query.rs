//! Query on-chain storage using sube.
//!
//! Run with: cargo run --example query --features wss,json,text

use sube::{sube, Response, Sube};

#[async_std::main]
async fn main() -> sube::Result<()> {
    let addr = "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b";

    // One-liner: connect, query and get the result in a single expression
    let response = sube(&format!("wss://kreivo.io/system/account/{addr}")).await?;
    print_value("Account (one-liner)", &response);

    // Reusable handle: connect once, query many times
    let chain = Sube::connect("wss://kreivo.io").await?;

    let response = chain.query(&format!("system/account/{addr}")).await?;
    print_value("Account (handle)", &response);

    // Query with the identity pallet — SuperOf maps AccountId32 to (AccountId32, Data)
    let response = chain.query(&format!("identity/superOf/{addr}")).await?;
    print_value("Identity SuperOf", &response);

    // Query a map with a u32 key (e.g. assets pallet)
    let response = chain.query("assets/asset/1984").await?;
    print_value("Asset 1984 (USDT)", &response);

    Ok(())
}

fn print_value(label: &str, response: &Response) {
    match response {
        Response::Value(entry, reg) => {
            // Display as text (compact, URL-safe format)
            let text = entry.to_text(reg).expect("valid text");
            println!("{label} (text): {text}");

            // Also display as JSON for comparison
            let json = entry.to_json(reg).expect("valid json");
            println!(
                "{label} (json): {}",
                serde_json::to_string_pretty(&json).unwrap()
            );
        }
        other => println!("{label}: {other:?}"),
    }
}
