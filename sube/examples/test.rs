#![feature(prelude_import)]
#[prelude_import]
use std::prelude::rust_2021::*;
#[macro_use]
extern crate std;
use futures_util::TryFutureExt;
use serde_json::json;
use libwallet::{self, vault};
use sube::sube;
use std::env;
use rand_core::OsRng;
type Wallet = libwallet::Wallet<vault::Simple>;
use anyhow::{Result, anyhow};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    async fn main() -> Result<(), Box<dyn std::error::Error>> {
        {
            let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");
            let (vault, phrase) = if phrase.is_empty() {
                vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
            } else {
                let phrase: libwallet::Mnemonic = phrase
                    .parse()
                    .expect("Invalid phrase");
                (vault::Simple::from_phrase(&phrase), phrase)
            };
            let mut wallet = Wallet::new(vault);
            wallet.unlock(None).await?;
            let account = wallet.default_account().public();
            let public = account.as_ref();

            let response = async {
                let mut builder = ::sube::builder::TxBuilder::default();
                // let account = &wallet.default_account();
                // let public = account.public();

                builder
                    .with_signer(|message: &[u8]| Ok(wallet.sign(message).into()))
                    .with_sender(public.into())
                    .with_body(
                        ::serde_json::Value::Object({
                            let mut object = ::serde_json::Map::new();
                            let _ = object
                                .insert(
                                    ("dest").into(),
                                    ::serde_json::Value::Object({
                                        let mut object = ::serde_json::Map::new();
                                        let _ = object
                                            .insert(
                                                ("Id").into(),
                                                ::serde_json::to_value(&public.as_ref()).unwrap(),
                                            );
                                        object
                                    }),
                                );
                            let _ = object
                                .insert(
                                    ("value").into(),
                                    ::serde_json::to_value(&100000).unwrap(),
                                );
                            object
                        }),
                    )
                    .await
            }
                .map_err(|err| anyhow!(format!("SubeError {:?}", err)))
                .await?;

            Ok(())
        }
    }
    async_std::task::block_on(async { main().await })
}