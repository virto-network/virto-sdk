use libwallet::{self, vault, Language};

use std::{env, error::Error};

type Wallet = libwallet::Wallet<vault::OSKeyring>;

const TEST_USER: &str = "test_user";

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let pin = env::args().nth(1);
    let pin = pin.as_ref().map(String::as_str);

    let vault = vault::OSKeyring::new(TEST_USER, Language::default());
    let mut wallet = Wallet::new(vault);
    wallet.unlock(pin).await?;

    let account = wallet.default_account();
    println!("Default account: {}", account);

    Ok(())
}
