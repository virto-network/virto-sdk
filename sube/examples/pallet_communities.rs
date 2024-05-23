use async_trait::async_trait;

use env_logger;
use serde_json;
use sube::{sube, ExtrinsicBody, Response, Result, SubeBuilder};

#[async_std::main]
async fn main() -> Result<()> {
    env_logger::init();

    log::info!("getting all the keys part of the subset");

    let response = sube!("ws://127.0.0.1:12281/communityMemberships/collection").await?;

    if let Response::ValueSet(value) = response {
        let data = serde_json::to_value(&value).expect("it must be a serialized object");
        println!("Account info: {}", serde_json::to_string_pretty(&data).expect("it must return an str"));
    }

    Ok(())
}
