//! Subscribe to chain events and react to new / finalized blocks.
//!
//! Demonstrates the `next_event` loop, querying storage at a specific
//! block hash, and distinguishing new-block from finalized-block events.
//!
//! Run with: cargo run --example subscribe --features wss

use sube::{ChainEvent, Sube};

fn main() -> sube::Result<()> {
    smol::block_on(async {
        let addr = "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b";

        let mut chain = Sube::connect("wss://kreivo.io").await?;
        println!("connected — watching 10 events\n");

        let mut seen = 0u32;
        while seen < 10 {
            match chain.next_event().await? {
                ChainEvent::NewBlock { hash, .. } => {
                    // Fetch a header at this block so we can print its number
                    let header = chain.header(&hash).await.ok();
                    let number = header.map(|h| h.number).unwrap_or(0);

                    // Query storage pinned to the same block hash
                    let account = chain
                        .query_at_hash(&format!("system/account/{addr}"), &hash)
                        .await?;
                    let text = account.to_text()?.unwrap_or_else(|| "(none)".into());
                    println!("new block #{number}: {text}");
                }
                ChainEvent::Finalized { hashes, .. } => {
                    println!("finalized {} block(s)", hashes.len());
                }
                ChainEvent::BestBlock { hash } => {
                    println!("best block → {hash}");
                }
            }
            seen += 1;
        }

        Ok(())
    })
}
