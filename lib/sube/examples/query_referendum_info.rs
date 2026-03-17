use sube::{Response, Result, Sube};

#[async_std::main]
async fn main() -> Result<()> {
    let chain = Sube::connect("wss://kreivo.io").await?;

    let query = format!("communityReferenda/referendumInfoFor/{}", 24);
    let r = chain.query(&query).await?;

    if let Response::Value(ref entry, reg) = r {
        let json_value = entry.to_json(reg)?;
        log::info!("Raw JSON value: {:?}", json_value);
        log::info!(
            "Info: {}",
            serde_json::to_string_pretty(&json_value).expect("it must return an str")
        );
    }
    Ok(())
}
