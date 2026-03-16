use sube::{sube, Response, Result};

#[async_std::main]
async fn main() -> Result<()> {
    let result = sube("ws://127.0.0.1:12281/system/account/0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b?at=2067321").await?;

    if let Response::Value(entry, reg) = result {
        let data = entry.to_json(reg)?;
        log::info!(
            "Account info: {}",
            serde_json::to_string_pretty(&data).expect("it must return an str")
        );
    }

    Ok(())
}
