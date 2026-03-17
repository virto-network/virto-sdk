use sube::{sube, Response, Sube};

#[async_std::main]
async fn main() -> sube::Result<()> {
    let chain = Sube::connect("ws://127.0.0.1:12281").await?;

    let query = format!(
        "preimage/preimageFor/{}/{}",
        "0x6b172c3695dca229e71c0bca790f5991b68f8eee96334e842312a0a7d4a46c6c", 30
    );

    let r = chain.query(&query).await?;

    if let Response::Value(ref entry, reg) = r {
        let json_value = entry.to_json(reg)?;
        log::info!("json: {:?}", json_value);
        let x = serde_json::to_string_pretty(&json_value).expect("it must return an str");
        log::info!("Preimage: {:?}", x);
    }

    Ok(())
}
