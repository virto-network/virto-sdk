use libwallet::{
    vault::{self, Vault},
    Language, Substrate, Wallet,
};

use std::error::Error;

const TEST_USER: &str = "test_user";

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let vault = vault::OSKeyring::<String>::new(TEST_USER, Language::default());
    let mut substrate = Substrate::new(vault);
    let signer = substrate.unlock(None, ()).await?;
    let mut wallet: Wallet<_, 5> = Wallet::new();
    wallet.add(signer);

    let account = wallet.default_account();
    println!("Default account: {}", account.unwrap().signer());

    Ok(())
}
