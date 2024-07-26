use core::future::{Future, IntoFuture};
use sube::{sube, Response, Result};

#[async_std::main]
async fn main() -> Result<()> {
    let result = sube!("wss://rococo-rpc.polkadot.io/system/account/0x3c85f79f28628bee75cdb9eddfeae249f813fad95f84120d068fbc990c4b717d").await?;

    println!("{:?}", result);
    Ok(())
}
