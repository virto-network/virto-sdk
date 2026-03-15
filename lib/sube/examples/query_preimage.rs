use sube::{sube, Response};

#[async_std::main]
async fn main() -> sube::Result<()> {
    let query = format!(
        "ws://127.0.0.1:12281/preimage/preimageFor/{}/{}",
        "0x6b172c3695dca229e71c0bca790f5991b68f8eee96334e842312a0a7d4a46c6c", 30
    );

    let r = sube!(&query).await?;

    if let Response::Value(ref entry, reg) = r {
        let json_value = entry.to_json(reg)?;
        println!("json: {:?}", json_value);
        let x = serde_json::to_string_pretty(&json_value).expect("it must return an str");
        println!("Preimage: {:?}", x);
    }

    Ok(())
}
