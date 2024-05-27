use env_logger;
use serde::Deserialize;
use sube::{sube, Error, ExtrinsicBody, Response, Result, SubeBuilder};

#[derive(Debug, Deserialize)]
pub struct AccountInfo {
    pub nonce: u64,
    pub consumers: u64,
    pub providers: u64,
    pub sufficients: u64,
    pub data: AccountData,
}

#[derive(Debug, Deserialize)]
pub struct AccountData {
    pub free: u128,
    pub reserved: u128,
    pub frozen: u128,
    pub flags: u128,
}

#[async_std::main]
async fn main() -> Result<()> {
    let response = SubeBuilder::default()
    .with_url("wss://rococo-rpc.polkadot.io/system/account/0x3c85f79f28628bee75cdb9eddfeae249f813fad95f84120d068fbc990c4b717d")
    .await?;

    if let Response::Value(v) = response {
        println!("{}", v);
    }

    Ok(())
}
