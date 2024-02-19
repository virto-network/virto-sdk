use async_trait::async_trait;

use sube::{builder::QueryBuilder, sube, ExtrinicBody, Response, Result, SignerFn};

#[async_std::main]
async fn main() -> Result<()> {
    let builder = QueryBuilder::default()
        .with_url("https://kusama.olanod.com/system/_constants/version")
        .await?;

    println!("{:?}", builder);
    Ok(())
}
