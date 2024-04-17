use futures_util::TryFutureExt;
use libwallet::{self, vault, Signature};
use rand_core::OsRng;
use serde_json::json;
use std::env;
use sube::sube;

type Wallet = libwallet::Wallet<vault::Simple>;

use anyhow::{anyhow, Result};

#[async_std::main]
async fn main() -> Result<()> {
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");

    let (vault, phrase) = if phrase.is_empty() {
        vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("Invalid phrase");
        (vault::Simple::from_phrase(&phrase), phrase)
    };

    let mut wallet = Wallet::new(vault);
    wallet
        .unlock(None)
        .await
        .map_err(|e| anyhow!("Error unlocking the wallet"))?;

    let account = wallet.default_account();
    let public = account.public();

    let response = sube!(
        "wss://rococo-rpc.polkadot.io/balances/transfer" =>
        (wallet, json!({
            "dest": {
                "Id": public.as_ref(),
            },
            "value": 100000
        }))
    )
    .await
    .map_err(|err| anyhow!(format!("SubeError {:?}", err)))?;

    println!("Secret phrase: \"{phrase}\"");
    // println!("Default Account: 0x{account}");

    Ok(())
}
