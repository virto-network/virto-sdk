use libwallet::{self, sr25519::Pair, OSVault};

use std::error::Error;

type Wallet = libwallet::Wallet<OSVault<Pair>>;

const TEST_USER: &str = "test_user";

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let user = std::env::args()
        .nth(1)
        .unwrap_or_else(|| TEST_USER.to_string());

    let (vault, phrase) = OSVault::<Pair>::new(&user).generate()?;
    let wallet = Wallet::new(vault).unlock(()).await?;
    let account = wallet.root_account()?;

    println!("Public key ({}): {}", account.network(), account);
    println!("Phrase: {phrase}");
    Ok(())
}
