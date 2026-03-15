use sube::{Result, SubeBuilder};

#[async_std::main]
async fn main() -> Result<()> {
    let response = SubeBuilder::default()
        .with_url("wss://rococo-rpc.polkadot.io/system/account/0x3c85f79f28628bee75cdb9eddfeae249f813fad95f84120d068fbc990c4b717d")
        .await?;

    log::info!("{:?}", response);
    Ok(())
}
