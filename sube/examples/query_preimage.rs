use core::future::{Future, IntoFuture};
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};
use sube::{sube, Response};

#[async_std::main]
async fn main() -> sube::Result<()> {
    env_logger::init();

    let query = format!(
        "ws://127.0.0.1:12281/preimage/preimageFor/{}/{}",
        "0x6b172c3695dca229e71c0bca790f5991b68f8eee96334e842312a0a7d4a46c6c", 30
    );

    let r = sube!(&query).await?;

    if let Response::Value(ref v) = r {
        let json_value = serde_json::to_value(v).expect("to be serializable");
        println!("json: {:?}", json_value);
        let x = serde_json::to_string_pretty(&json_value).expect("it must return an str");
        println!("Account info: {:?}", x);
    }

    Ok(())
}
