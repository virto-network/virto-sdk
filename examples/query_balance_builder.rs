use async_trait::async_trait;

use sube::{ Response, sube, Result, builder::QueryBuilder, SignerFn, ExtrinicBody };

#[async_std::main]
async fn main() -> Result<()> {
    let builder = QueryBuilder::default()
        .with_url("https://kusama.olanod.com/system/_constants/version")
        .await?;

    println!("{:?}", builder);
    Ok(())
}
