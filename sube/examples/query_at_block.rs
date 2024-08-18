use env_logger;
use serde_json;
use sube::{sube, ExtrinsicBody, Response, Result, SubeBuilder};

#[async_std::main]
async fn main() -> Result<()> {
    env_logger::init();

    let result = sube!("ws://127.0.0.1:12281/system/account/0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b?at=0x8c0eb4368ffcc1fca8226b1653a4b3ba50d22fe494dab1dac3df206d438c7e70").await?;

    if let Response::Value(value) = result {
        let data = serde_json::to_value(&value).expect("to be serializable");
        println!(
            "Account info: {}",
            serde_json::to_string_pretty(&data).expect("it must return an str")
        );
    }

    let result = sube!("ws://127.0.0.1:12281/system/account/0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b?at=2062650").await?;

    if let Response::Value(value) = result {
        let data = serde_json::to_value(&value).expect("to be serializable");
        println!(
            "Account info: {}",
            serde_json::to_string_pretty(&data).expect("it must return an str")
        );
    }

    Ok(())
}
