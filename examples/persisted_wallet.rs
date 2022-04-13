use libwallet::{self, sr25519::Pair, OSVault};

use std::error::Error;

type Wallet = libwallet::Wallet<OSVault<Pair>>;

const MNEMONIC: &str = "caution juice atom organ advance problem want pledge someone senior holiday very";
// key example taken from https://docs.substrate.io/v3/tools/subkey/#hd-key-derivation
// 5Gv8YYFu8H1btvmrJy9FjjAWfb99wrhV3uhPFoNEr918utyR

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let account_name = "main";

    let network: &str = "substrate";

    let vault = libwallet::OSVault::<Pair>::create_with_seed(account_name, MNEMONIC).unwrap();
    let mut wallet = Wallet::new(vault).unlock(()).await?;
    let account = wallet.switch_default_network(network)?;

    println!("Public key ({}): {}", account.network(), account);

    let copy_vault = libwallet::OSVault::<Pair>::new(account_name);
    let mut wallet = Wallet::new(copy_vault).unlock(()).await?;
    let account = wallet.switch_default_network(network)?;


    println!("Public key ({}): {}", account.network(), account);

    Ok(())
}

