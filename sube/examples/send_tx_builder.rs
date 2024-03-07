use blake2::digest::crypto_common::rand_core;
use jsonrpc::error;
use libwallet::{self, vault, Signature};
use rand_core::OsRng;
use serde_json::json;
use std::env;
use sube::builder::{QueryBuilder, TxBuilder};

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

    wallet.unlock(None).await?;

    let account = wallet.default_account();
    let w = wallet.default_account().public();
    let x = wallet.default_account().public();
    let y = format!("0x{}", hex::encode(wallet.default_account().public()));
    println!("{:?}", y);
    let response = TxBuilder::default()
        .with_url("wss://rococo-rpc.polkadot.io/balances/transfer_Keep_Alive")
        .with_signer(|message: &[u8]| Ok(wallet.sign(message).as_bytes()))
        .with_sender(x.as_ref())
        .with_body(json!({
            "dest": {
                "Id": w.as_ref()
            },
            "value": 2_000_000_000
        }))
        .await
        .map_err(|err| anyhow!(format!("Error {:?}", err)))?;

    println!("{:?}", response);
    Ok(())
}
