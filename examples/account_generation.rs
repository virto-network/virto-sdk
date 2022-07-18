use libwallet::{self, vault};
use std::env;

type Wallet = libwallet::Wallet<vault::Simple>;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");

    let (vault, phrase) = if phrase.is_empty() {
        vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("Invalid phrase");
        (vault::Simple::from_phrase(&phrase), phrase)
    };

    let mut wallet = Wallet::new(vault);
    wallet.unlock(()).await?;
    let account = wallet.default_account();

    println!("Secret phrase: \"{phrase}\"");
    println!("Default Account: 0x{account}");
    Ok(())
}
