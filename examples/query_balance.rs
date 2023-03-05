


use sube::{ builder::SubeBuilder as Sube };
use async_trait::async_trait;

#[async_std::main]
async fn main () {
    let a = Sube("wss://rpc.polkadot.io").await?;
}
