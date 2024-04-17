use jsonrpc::error;
use serde_json::json;
use libwallet::{self, vault, Signature};
use sube::builder::TxBuilder;
use std::env;
use rand_core::OsRng;

type Wallet = libwallet::Wallet<vault::Simple>;
use anyhow::{ Result, anyhow };

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

    wallet.unlock(None).await?;

    let account = wallet.default_account();

    let response = TxBuilder::default()
        .with_url("https://kusama.olanod.com/balances/transfer")
        .with_signer(|message: &[u8]|  Ok(wallet.sign(message).as_bytes()) )
        .with_sender(wallet.default_account().public().as_ref())
        .with_body(json!({
            "dest": {
                "Id": wallet.default_account().public().as_ref()
            },
            "value": 100000
        }))
        .await
        .map_err(|err| {
            anyhow!(format!("Error {:?}", err))
        })?;

    Ok(())
}