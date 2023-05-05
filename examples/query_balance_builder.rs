use async_trait::async_trait;

use sube::{ Response, sube, Result, builder::SubeBuilder, SignerFn, ExtrinicBody };

#[async_std::main]
async fn main() -> Result<()> {
    let builder = SubeBuilder::default()
        .with_url("https://kusama.olanod.com/system/_constants/version")
        .await?;

    println!("{:?}", builder);
    Ok(())
}
