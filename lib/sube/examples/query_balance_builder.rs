use sube::{sube, Result, Sube};

#[async_std::main]
async fn main() -> Result<()> {
    // One-liner
    let response = sube("wss://kreivo.io/system/account/0x3c85f79f28628bee75cdb9eddfeae249f813fad95f84120d068fbc990c4b717d").await?;
    log::info!("{:?}", response);

    // Reusable handle
    let chain = Sube::connect("wss://kreivo.io").await?;
    let response = chain.query("system/account/0x3c85f79f28628bee75cdb9eddfeae249f813fad95f84120d068fbc990c4b717d").await?;
    log::info!("{:?}", response);

    Ok(())
}
