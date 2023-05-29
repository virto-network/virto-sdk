use async_trait::async_trait;
use core::future::{Future, IntoFuture};
use sube::{ Response, sube, Result };

#[async_std::main]
async fn main() -> Result<()> {
    
    let result = sube!("https://kusama.olanod.com/system/_constants/version").await?;

    println!("{:?}", result);
    Ok(())
}
