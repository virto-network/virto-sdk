use sube::{sube, Response};

#[async_std::main]
async fn main() -> sube::Result<()> {
    let result = sube!("ws://localhost:11004/identity/superOf/0x6d6f646c6b762f636d7479738501000000000000000000000000000000000000").await?;

    if let Response::Value(entry, reg) = result {
        let data = entry.to_json(reg)?;
        log::info!(
            "Account info: {}",
            serde_json::to_string_pretty(&data).expect("it must return an str")
        );
    }

    let query = "ws://localhost:11004/identity/identityOf/0xbe6ed76ac48d5c7f1c5d2cab8a1d1e7a451dcc24b624b088ef554fd47ba21139";

    let r = sube!(query).await?;

    if let Response::Value(ref entry, reg) = r {
        let json_value = entry.to_json(reg)?;
        log::info!("json: {:?}", json_value);
        let x = serde_json::to_string_pretty(&json_value).expect("it must return an str");
        log::info!("Account info: {:?}", x);
    }

    Ok(())
}
