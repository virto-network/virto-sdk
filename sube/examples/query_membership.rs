use core::future::{Future, IntoFuture};
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};
use sube::{sube, Response};

#[async_std::main]
async fn main() -> sube::Result<()> {
    env_logger::init();

    let query = format!(
        "ws://127.0.0.1:12281/communityMemberships/account/{}/{}",
        "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b",
        1
    );
    
    let r = sube!(&query).await?;

    if let Response::ValueSet(ref v) = r {
        let json_value = serde_json::to_value(v).expect("to be serializable");
        println!("json: {:?}", json_value);
        let x = serde_json::to_string_pretty(&json_value).expect("it must return an str");
        println!("Account info: {:?}", x);
    }

    Ok(())
}