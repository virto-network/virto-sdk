//! Submit an extrinsic (transaction) to a chain.
//!
//! Submissions are tracked via `transactionWatch_v1_submitAndWatch`
//! and wait for finalization before returning.
//!
//! Run with: cargo run --example submit --features wss,examples -- [seed phrase]

use libwallet::{
    self,
    vault::{utils::DerivedSigner, Simple, Vault as _},
    Signer as _, Substrate,
};
use std::env;
use sube::{SignerFn, Sube};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    smol::block_on(async {
        let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");
        if phrase.is_empty() {
            eprintln!("Usage: submit <seed phrase>");
            std::process::exit(1);
        }
        let keys = Simple::<()>::from_phrase(&phrase).map_err(|_| "invalid phrase")?;

        // Derive a substrate signer from the key store
        let mut vault = Substrate::new(keys);
        let account: DerivedSigner = vault
            .unlock(None, ())
            .await
            .map_err(|_| "vault unlock failed")?;
        let account_id: [u8; 32] = account.public().as_ref().try_into().unwrap();

        println!("Account: 0x{}", hex::encode(account_id));
        println!("Phrase: \"{phrase}\"");

        // Adapt libwallet::Signer into sube::SignerFn
        let signer = SignerFn::from((&account_id, |message: &[u8]| {
            let msg = message.to_vec();
            let account = &account;
            async move {
                account
                    .sign_msg(&msg)
                    .await
                    .map(|sig| sig.as_ref().try_into().unwrap())
                    .map_err(|_| sube::Error::Encode("signing failed".into()))
            }
        }));

        let mut chain = Sube::connect("wss://kreivo.io").await?;

        // Submit using scales text format body
        chain
            .call("system/remark")
            .body_text("(remark:0x68656c6c6f)")
            .signer(&signer)
            .await?;
        println!("Submitted remark");

        // Text format also handles complex types like enum arguments
        let dest = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
        let body = format!("(dest:MultiAddress::Id({dest});value:1000000000000)");
        chain
            .call("balances/transfer_keep_alive")
            .body_text(&body)
            .signer(&signer)
            .await?;
        println!("Submitted transfer");

        Ok(())
    })
}
