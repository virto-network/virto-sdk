use core::future::{Future, IntoFuture};
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};
use sube::{sube, Response};

#[async_std::main]
async fn main() -> sube::Result<()> {
    env_logger::init();

    let query = format!("ws://127.0.0.1:11004/identity/identityOf/0xbe6ed76ac48d5c7f1c5d2cab8a1d1e7a451dcc24b624b088ef554fd47ba21139");
    
    let r = sube!(&query).await?;

    if let Response::Value(ref v) = r {
        let json_value = serde_json::to_value(v).expect("to be serializable");
        println!("json: {:?}", json_value);
        let x = serde_json::to_string_pretty(&json_value).expect("it must return an str");
        println!("Account info: {:?}", x);
    }

    Ok(())
}