use async_trait::async_trait;
use sube::builder::sube;
use sube::Response;

#[async_std::main]
async fn main() {
    let a: Response = 
        sube("https://kusama.olanod.com/system/_constants/version")
        
        .await?;

    println!("{:?} ", a);
}
