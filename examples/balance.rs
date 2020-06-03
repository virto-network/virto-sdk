use libwallet::{chain::Chain, Account};
use sp_core::sr25519::Pair;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let alice: Account<Pair> = "//Alice".into();
    let bob: Account<Pair> = "//Bob".into();
    println!(
        "Will transfer from Alice({}) to Bob({})",
        alice.id(),
        bob.id()
    );
    let chain = Chain::connect("wss://test.valibre.network").await?;
    println!("{}", chain.name().await);
    Ok(())
}
