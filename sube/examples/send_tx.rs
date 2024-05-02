#![feature(async_closure)]

use futures_util::TryFutureExt;
use libwallet::{self, vault, Account, Signature};
use rand_core::OsRng;
use serde_json::json;
use std::env;
use sube::sube;

type Wallet = libwallet::Wallet<vault::Simple<String>>;

use anyhow::{anyhow, Result};

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init(); 
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");

    let (vault, phrase) = if phrase.is_empty() {
        vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("Invalid phrase");
        (vault::Simple::from_phrase(&phrase), phrase)
    };

    let mut wallet = Wallet::new(vault);
    wallet
        .unlock(None, None)
        .await
        .map_err(|_| anyhow!("error wallet"))?;

    let account = wallet.default_account();
    let public = account.unwrap().public();
    let public_hex = public.as_ref();
    println!("hex: {}", hex::encode(public_hex));

    let response = sube!("ws://127.0.0.1:8000/balances/transfer_keep_alive" => {
        signer: async |message: &[u8]| Ok(wallet.sign(message).await.expect("hello").as_bytes()) ,
        sender: public.as_ref(),
        body:  json!({      
            "dest": {
                "Id": hex::decode("e25b1e3758a5fbedb956b36113252f9e866d3ece688364cc9d34eb01f4b2125d").unwrap()
            },
            "value": 1000000000
        }),
    })
    .await
    .map_err(|err| anyhow!(format!("SubeError {:?}", err)))?;

    println!("response: {:?}", response);
    println!("Secret phrase: \"{phrase}\"");
    println!("Default Account: 0x{}", account.unwrap());

    Ok(())
}
