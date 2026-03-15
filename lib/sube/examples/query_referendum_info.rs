use sube::{sube, Response, Result};

#[async_std::main]
async fn main() -> Result<()> {
    let query = format!(
        "https://kreivo.io/communityReferenda/referendumInfoFor/{}",
        24
    );

    let r = sube!(&query).await?;

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
