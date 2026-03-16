use sube::{sube, Result};

#[async_std::main]
async fn main() -> Result<()> {
    let response = sube("wss://kreivo.io/system/account/0x3c85f79f28628bee75cdb9eddfeae249f813fad95f84120d068fbc990c4b717d").await?;

    log::info!("{:?}", response);
    Ok(())
}
