//! Explore storage maps and historical state.
//!
//! Run with: cargo run --example explore --features wss,json

use sube::{Response, Sube};

fn main() -> sube::Result<()> {
    smol::block_on(async {
        let mut chain = Sube::connect("wss://kreivo.io").await?;

        // Iterate over all entries of a storage map
        let response = chain.query("communityMemberships/collection").await?;
        if let Response::ValueSet(entries, meta) = response {
            for (keys, value) in &entries {
                for key in keys {
                    let k = key.to_json(&meta.registry)?;
                    println!("key: {k:?}");
                }
                if let Some(val) = value {
                    let v = val.to_json(&meta.registry)?;
                    println!("val: {v:?}");
                }
            }
            println!("total entries: {}", entries.len());
        }

        // Query state at a specific block number
        let addr = "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b";
        let response = chain
            .query_at(&format!("system/account/{addr}"), 2067321)
            .await?;
        if let Response::Value(entry, meta) = response {
            let data = entry.to_json(&meta.registry)?;
            println!(
                "Account at block 2067321: {}",
                serde_json::to_string_pretty(&data).unwrap()
            );
        }

        Ok(())
    })
}
