use libwallet::{self, vault, Account};
use serde_json::json;
use std::{env, error::Error};
use sube::{Bytes, SignerFn, Sube};

type Wallet = libwallet::Wallet<vault::Simple<String>>;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");

    let (vault, _phrase) = if phrase.is_empty() {
        vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("Invalid phrase");
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
                let result = wallet
                    .sign(&message)
                    .await
                    .map(|signature| signature.as_ref().try_into().unwrap())
                    .map_err(|_| sube::Error::Signing)?;

                Ok::<Bytes<64>, sube::Error>(result)
            }
        },
    ));

    let chain = Sube::connect("wss://kreivo.io").await
        .map_err(|e| format!("Failed to connect: {e}"))?;

    let response = chain.call("balances/transfer")
        .body(json!({
            "dest": {
                "Id": account.public().as_ref()
            },
            "value": 100000
        }))
        .signer(signer)
        .await
        .map_err(|e| format!("Failed to send tx: {e}"));

    log::info!("{:?}", response);
    Ok(())
}
