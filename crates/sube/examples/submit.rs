//! Submit an extrinsic (transaction) to a chain.
//!
//! Submissions are tracked via `transactionWatch_v1_submitAndWatch`
//! and wait for finalization before returning.
//!
//! Run with: cargo run --example submit --features wss,examples -- [seed phrase]

use libwallet::{
    self, Signer as _, Substrate,
    vault::{Simple, Vault as _, utils::DerivedSigner},
};
use std::env;
use sube::{SignerFn, Sube, Text, TransactionOptions, WaitFor};

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
        let signer = SignerFn::new(account_id, |message: &[u8]| {
            let msg = message.to_vec();
            let account = &account;
            async move {
                account
                    .sign_msg(&msg)
                    .await
                    .map(|sig| sig.as_ref().try_into().unwrap())
                    .map_err(|_| sube::Error::Encode("signing failed".into()))
            }
        });

        let mut chain = Sube::connect("wss://kreivo.io").await?;

        // Preparation and building are non-mutating. Only the final explicit
        // submit call sends bytes to the chain.
        let call = chain.prepare_call("system/remark", &Text("(remark:0x68656c6c6f)"))?;
        let extrinsic = chain
            .build_transaction(&call, &signer, TransactionOptions::default())
            .await?;
        println!("Call: {}", call.hex);
        println!("Extrinsic: {}", extrinsic.hex);
        let receipt = chain
            .submit_transaction(&extrinsic, WaitFor::Finalized)
            .await?;
        println!(
            "Submitted remark in {} at index {:?}",
            receipt
                .finalized_block_hash
                .as_deref()
                .unwrap_or("unknown block"),
            receipt.extrinsic_index
        );

        // Text format also handles complex types like enum arguments
        let dest = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
        let body = format!("(dest:MultiAddress::Id({dest});value:1000000000000)");
        let call = chain.prepare_call("balances/transfer_keep_alive", &Text(&body))?;
        let extrinsic = chain
            .build_transaction(&call, &signer, TransactionOptions::default())
            .await?;
        let receipt = chain
            .submit_transaction(&extrinsic, WaitFor::Finalized)
            .await?;
        println!(
            "Submitted transfer in {}",
            receipt
                .finalized_block_hash
                .as_deref()
                .unwrap_or("unknown block")
        );

        Ok(())
    })
}
