//! Query on-chain storage using sube.
//!
//! Run with: cargo run --example query --features wss,text

use sube::Sube;

fn main() -> sube::Result<()> {
    smol::block_on(async {
        let addr = "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b";

        // One-liner: connect, query and get the result in a single expression
        let response = sube::sube(&format!("wss://kreivo.io/system/account/{addr}")).await?;
        if let Some(text) = response.to_text()? {
            println!("Account (one-liner): {text}");
        }

        // Reusable handle: connect once, query many times
        let mut chain = Sube::connect("wss://kreivo.io").await?;

        let response = chain.query(&format!("system/account/{addr}")).await?;
        if let Some(text) = response.to_text()? {
            println!("Account (text): {text}");
        }

        // Query a constant
        let response = chain.query("system/_constants/Version").await?;
        if let Some(text) = response.to_text()? {
            println!("Version: {text}");
        }

        Ok(())
    })
}
