use sube::{sube, Response, Result, Sube};

#[async_std::main]
async fn main() -> Result<()> {
    let kreivo = Sube::connect("wss://kreivo.io").await?;
    let response = kreivo.query("communityMemberships/account/0xe25b1e3758a5fbedb956b36113252f9e866d3ece688364cc9d34eb01f4b2125d/2").await.expect("to work");

    if let Response::ValueSet(entries, reg) = response {
        for (keys, value) in &entries {
            for key in keys {
                let json_key = key.to_json(reg)?;
                log::info!("key: {:?}", json_key);
            }
            if let Some(val) = value {
                let json_value = val.to_json(reg)?;
                log::info!("Collection Array value: {:?}", json_value);
            }
        }
    }

    let local = Sube::connect("ws://127.0.0.1:12281").await?;
    let response = local.query("communityMemberships/collection").await?;

    if let Response::ValueSet(entries, reg) = response {
        for (keys, value) in &entries {
            for key in keys {
                let json_key = key.to_json(reg)?;
                log::info!("key: {:?}", json_key);
            }
            if let Some(val) = value {
                let json_value = val.to_json(reg)?;
                log::info!("Collection value: {:?}", json_value);
            }
        }
    }

    let result = sube("https://kreivo.io/system/account/0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b").await?;

    if let Response::Value(entry, reg) = result {
        let data = entry.to_json(reg)?;
        log::info!(
            "Account info: {}",
            serde_json::to_string_pretty(&data).expect("it must return an str")
        );
    }

    Ok(())
}
