use dirs::home_dir;
use libwallet::{self, vault::Pass, Language};
use std::error::Error;
type Wallet = libwallet::Wallet<Pass>;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // first argument is used as account
    let account = std::env::args().skip(1).next().unwrap_or("default".into());
    let mut store_path = home_dir().expect("Could not find home path");
    store_path.push(".password-store");

    let vault = Pass::new(store_path.to_str().unwrap(), Language::default());
    let mut wallet = Wallet::new(vault);
    wallet.unlock(account).await?;

    let account = wallet.default_account();
    println!("Default account: {}", account);

    Ok(())
}
