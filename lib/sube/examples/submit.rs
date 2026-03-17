//! Submit an extrinsic (transaction) to a chain.
//!
//! Run with: cargo run --example submit --features wss,json,examples -- [seed phrase]

use libwallet::{self, vault, Account};
use serde_json::json;
use std::env;
use sube::{SignerFn, Sube};

type Wallet = libwallet::Wallet<vault::Simple<String>>;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");
    let (vault, phrase) = if phrase.is_empty() {
        vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("invalid seed phrase");
        (vault::Simple::from_phrase(&phrase), phrase)
    };

    let mut wallet = Wallet::new(vault);
    wallet.unlock(None, None).await?;
    let account = wallet.default_account().unwrap();

    let signer = SignerFn::from((
        account.public().as_ref(),
        |message: &[u8]| {
            let message = message.to_vec();
            let wallet = &wallet;
            async move {
                wallet
                    .sign(&message)
                    .await
                    .map(|sig| sig.as_ref().try_into().unwrap())
                    .map_err(|_| sube::Error::Signing)
            }
        },
    ));

    println!("Account: 0x{account}");
    println!("Phrase: \"{phrase}\"");

    let chain = Sube::connect("wss://kreivo.io").await?;
    let _response = chain
        .call("system/remark")
        .body(json!({ "remark": "0x68656c6c6f" }))
        .signer(signer)
        .await?;

    println!("Extrinsic submitted");
    Ok(())
}
