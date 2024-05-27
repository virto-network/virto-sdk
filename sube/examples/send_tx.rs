use futures_util::TryFutureExt;
use libwallet::{self, vault, Account, Signature};
use rand_core::OsRng;
use serde_json::json;
use std::env;
use sube::{sube, Error};

type Wallet = libwallet::Wallet<vault::Simple<String>>;

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
    wallet.unlock(None, None).await?;

    let account = wallet.default_account();
    let public = account.unwrap().public();

    let response = sube!("wss://rococo-rpc.polkadot.io/balances/transfer" => {
        body: json!({
            "dest": { "Id": public.as_ref() },
            "value": 100000
        }),
        signer: (public, |message: &[u8]| async {
            Ok::<_, Error>(wallet.sign(message).await.map_err(|_| Error::Signing)?)
        }),
    })
    .await?;

    println!("Secret phrase: \"{phrase}\"");
    println!("Default Account: 0x{}", account.unwrap());

    Ok(())
}
