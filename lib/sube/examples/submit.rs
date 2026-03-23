//! Submit an extrinsic (transaction) to a chain.
//!
//! Shows two ways to encode the call body: JSON and scales text format.
//! Submissions are tracked via `transactionWatch_v1_submitAndWatch`
//! and wait for finalization before returning.
//!
//! Run with: cargo run --example submit --features wss,json,text,examples -- [seed phrase]

use libwallet::{self, vault, Account};
use serde_json::json;
use std::env;
use sube::{SignerFn, Sube};

type Wallet = libwallet::Wallet<vault::Simple<String>>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    smol::block_on(async {
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

        let signer = SignerFn::from((account.public().as_ref(), |message: &[u8]| {
            let message = message.to_vec();
            let wallet = &wallet;
            async move {
                wallet
                    .sign(&message)
                    .await
                    .map(|sig| sig.as_ref().try_into().unwrap())
                    .map_err(|_| sube::Error::Encode("signing failed".into()))
            }
        }));

        println!("Account: 0x{account}");
        println!("Phrase: \"{phrase}\"");

        let mut chain = Sube::connect("wss://kreivo.io").await?;

        // Submit using JSON body
        chain
            .call("system/remark")
            .body(json!({ "remark": "0x68656c6c6f" }))
            .signer(&signer)
            .await?;
        println!("Submitted remark (JSON body)");

        // Submit using text format body
        chain
            .call("system/remark")
            .body_text("(remark:0x68656c6c6f)")
            .signer(&signer)
            .await?;
        println!("Submitted remark (text body)");

        // Text format with complex types like enum arguments
        let dest = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
        let body = format!("(dest:MultiAddress::Id({dest});value:1000000000000)");
        chain
            .call("balances/transfer_keep_alive")
            .body_text(&body)
            .signer(&signer)
            .await?;
        println!("Submitted transfer (text body with enum)");

        Ok(())
    })
}
