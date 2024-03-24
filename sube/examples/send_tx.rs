use futures_util::TryFutureExt;
use serde_json::json;
use libwallet::{self, vault};
use sube::sube;
use std::env;
use rand_core::OsRng;

type Wallet = libwallet::Wallet<vault::Simple>;

use anyhow::{ Result, anyhow };

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
    wallet.unlock(None).await?;

    let account = wallet.default_account();
    let public = account.public();

    let response = sube!("https://kusama.olanod.com/balances/transfer" => {
        signer: |message: &[u8]| Ok(wallet.sign(message).into()),
        sender: public.as_ref(),
        body:  json!({
            "dest": {
                "Id": public.as_ref()
            },
            "value": 100000
        }),
    })
    .map_err(|err| anyhow!(format!("SubeError {:?}", err)))
    .await?;


    println!("Secret phrase: \"{phrase}\"");
    println!("Default Account: 0x{account}");

    Ok(())
}