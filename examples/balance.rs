use libwallet::chain::Chain;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let chain = Chain::connect("wss://test.valibre.network").await?;
    println!("{}", chain.name().await);
    Ok(())
}
