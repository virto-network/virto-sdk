use async_trait::async_trait;
use core::future::{Future, IntoFuture};
use sube::{ Response, sube, Result, builder::SubeBuilder };

#[async_std::main]
async fn main() -> Result<()> {
    
    sube!("https://kusama.olanod.com/system/_constants/version").await?;

    Ok(())
}
