use sube::{sube, Response, Sube};

#[async_std::main]
async fn main() -> sube::Result<()> {
    let chain = Sube::connect("ws://127.0.0.1:12281").await?;

    let query = format!(
        "communityMemberships/account/{}/{}",
        "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b", 1
    );

    let r = chain.query(&query).await?;

    if let Response::ValueSet(ref entries, reg) = r {
        for (keys, value) in entries {
            for key in keys {
                let json_key = key.to_json(reg)?;
                log::info!("key: {:?}", json_key);
            }
            if let Some(val) = value {
                let json_value = val.to_json(reg)?;
                log::info!("value: {:?}", json_value);
            }
        }
    }

    Ok(())
}
