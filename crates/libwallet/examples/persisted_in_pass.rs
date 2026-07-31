use dirs::home_dir;
use libwallet::{
    vault::{Pass, Vault},
    Language, Substrate, Wallet,
};
use std::error::Error;
type PassVault = Pass<String>;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // first argument is used as account
    let account = std::env::args().nth(1).unwrap_or("default".into());
    let mut store_path = home_dir().expect("Could not find home path");
    store_path.push(".password-store");

    let vault: PassVault =
        Pass::new(store_path.to_str().unwrap(), Language::default()).account(&account);
    let mut substrate = Substrate::new(vault);
    let signer = substrate.unlock(None, ()).await?;
    let mut wallet: Wallet<_, 5> = Wallet::new();
    wallet.add(signer);

    let account = wallet.default_account();
    println!("Default account: {}", account.unwrap().signer());

    Ok(())
}
