use libwallet::{self, vault, Account, Signature};
use serde_json::json;
use std::{env, error::Error};
use sube::{Bytes, SubeBuilder, Signer};
type Wallet = libwallet::Wallet<vault::Simple<String>>;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");

    let (vault, phrase) = if phrase.is_empty() {
        vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("Invalid phrase");
        (vault::Simple::from_phrase(&phrase), phrase)
    };

    let mut wallet = Wallet::new(vault);
    wallet.unlock(None, None).await?;

    let account = wallet.default_account().unwrap();

    let signer = sube::SignerFn::from((
        account.public().as_ref(),
        |message: &[u8]| {
            let message = message.to_vec();
            let wallet = &wallet;

            async move {
                let result = wallet
                    .sign(&message)
                    .await
                    .map(|signature| signature.as_ref().try_into().unwrap())
                    .map_err(|_| sube::Error::Signing)?;

                Ok::<Bytes<64>, sube::Error>(result)
            }
        },
    ));

    let response = SubeBuilder::default()
        .with_url("wss://rococo-rpc.polkadot.io/balances/transfer")
        .with_body(json!({
            "dest": {
                "Id": account.public().as_ref()
            },
            "value": 100000
        }))
        .with_signer(signer)
        .await.map_err(|_| format!("Failed to send tx"));

    println!("{:?}", response);
    Ok(())
}

