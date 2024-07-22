use env_logger;
use serde_json;
use sube::{sube, ExtrinsicBody, Response, Result, SubeBuilder};

#[async_std::main]
async fn main() -> Result<()> {
    env_logger::init();

    let query = format!(
        "https://kreivo.io/communityReferenda/referendumInfoFor/{}",
        24
    );

    let r = sube!(&query).await?;

    if let Response::Value(ref v) = r {
        let json_value = serde_json::to_value(v).expect("it must to be an valid Value");
        println!("Raw JSON value: {:?}", json_value);
        println!("Info: {}", serde_json::to_string_pretty(&json_value).expect("it must return an str"));
    }
    Ok(())
}
