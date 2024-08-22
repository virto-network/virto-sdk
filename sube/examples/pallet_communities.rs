use env_logger;
use futures_util::future::join_all;
use serde_json;
use sube::{sube, ExtrinsicBody, Response, Result, SubeBuilder};


#[async_std::main]
async fn main() -> Result<()> {
    env_logger::init();

    let response = sube!("wss://kreivo.io/communityMemberships/account/0xe25b1e3758a5fbedb956b36113252f9e866d3ece688364cc9d34eb01f4b2125d/2").await.expect("to work");

    if let Response::ValueSet(value) = response {
        let data = serde_json::to_value(&value).expect("to be serializable");
        println!(
            "Collection Array {}",
            serde_json::to_string_pretty(&data).expect("it must return an str")
        );
    }

    // let response = sube!("ws://127.0.0.1:12281/communityMemberships/collection").await?;

    // if let Response::ValueSet(value) = response {
    //     let data = serde_json::to_value(&value).expect("to be serializable");
    //     println!(
    //         "Collection {}",
    //         serde_json::to_string_pretty(&data).expect("it must return an str")
    //     );
    // }

    // let result = sube!("https://kreivo.io/system/account/0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b").await?;

    // if let Response::Value(value) = result {
    //     let data = serde_json::to_value(&value).expect("to be serializable");
    //     println!(
    //         "Account info: {}",
    //         serde_json::to_string_pretty(&data).expect("it must return an str")
    //     );
    // }

    Ok(())
}
