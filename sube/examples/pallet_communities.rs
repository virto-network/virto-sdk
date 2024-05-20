use async_trait::async_trait;

use env_logger;
use serde_json;
use sube::{sube, ExtrinsicBody, Response, Result, SubeBuilder};

#[async_std::main]
async fn main() -> Result<()> {
    env_logger::init();

    let response = sube!("ws://127.0.0.1:8000/communityTracks/tracks/1").await?;

     if let Response::Value(value) = response {
        let data = serde_json::to_value(&value).expect("it must be a serialized object");
        println!("Account info: {}", serde_json::to_string_pretty(&data).expect("it must return an str"));
    }

    Ok(())
}
