use libwallet::{
    vault::{self, Vault},
    Substrate, Wallet,
};
use std::env;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");

    let (vault, _phrase) = if phrase.is_empty() {
        vault::Simple::<String>::generate_with_phrase(&mut rand_core::OsRng)?
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("Invalid phrase");
        (vault::Simple::<String>::from_phrase(&phrase)?, phrase)
    };

    let mut substrate = Substrate::new(vault);
    let signer = substrate
        .unlock(None, ())
        .await
        .map_err(|_| "Failed to unlock vault")?;
    let mut wallet: Wallet<_, 5> = Wallet::new();
    wallet.add(signer);
    let account = wallet.default_account().unwrap();

    // NOTE: Never print mnemonic phrases in production code.
    // The phrase is kept only for backup — store securely, never log.
    println!("Default Account: 0x{}", account.signer());
    Ok(())
}
