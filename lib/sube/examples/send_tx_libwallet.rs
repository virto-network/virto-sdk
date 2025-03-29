use libwallet::{self, vault, Account, Signature};
use rand_core::OsRng;
use serde_json::json;
use std::{env, error::Error};
use sube::sube;

type Wallet = libwallet::Wallet<vault::Simple<String>>;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");

    let (vault, phrase) = if phrase.is_empty() {
        vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("Invalid phrase");
        (vault::Simple::from_phrase(&phrase), phrase)
    };

    let mut wallet = Wallet::new(vault);
    wallet.unlock(None, None).await?;

    let account = wallet.default_account().unwrap();

    let response = sube!(
        "wss://rococo-rpc.polkadot.io/balances/transfer" =>
        (wallet, json!({
            "dest": {
                "Id": account.public().as_ref(),
            },
            "value": 100000
        }))
    )
    .await.map_err(|_| format!("Error sending tx"))?;

    println!("Secret phrase: \"{phrase}\"");
    println!("Default Account: 0x{account}");

    Ok(())
}
