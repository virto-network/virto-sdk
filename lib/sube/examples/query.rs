//! Query on-chain storage using sube.
//!
//! Run with: cargo run --example query --features wss,json

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

    let response = chain.query(&format!("identity/superOf/{addr}")).await?;
    print_value("Identity", &response);

    Ok(())
}

fn print_value(label: &str, response: &Response) {
    match response {
        Response::Value(entry, reg) => {
            let json = entry.to_json(reg).expect("valid json");
            println!("{label}: {}", serde_json::to_string_pretty(&json).unwrap());
        }
        other => println!("{label}: {other:?}"),
    }
}
