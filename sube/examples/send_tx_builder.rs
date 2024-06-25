use jsonrpc::error;
use libwallet::{self, vault, Account, Signature};
use rand_core::OsRng;
use serde_json::json;
use std::{env, rc::Rc};
use sube::{Error, Signer, SignerFn, SubeBuilder};

type Wallet = libwallet::Wallet<vault::Simple<String>>;

#[async_std::main]
async fn main() -> Result<(), Error> {
    let phrase = env::args().skip(1).collect::<Vec<_>>().join(" ");

    let (vault, phrase) = if phrase.is_empty() {
        vault::Simple::generate_with_phrase(&mut rand_core::OsRng)
    } else {
        let phrase: libwallet::Mnemonic = phrase.parse().expect("Invalid phrase");
        (vault::Simple::from_phrase(&phrase), phrase)
    };

    let mut wallet = Wallet::new(vault);
    wallet.unlock(None, None).await.expect("unlocked");
  
    let account = wallet.default_account().unwrap();

    let signer = sube::SignerFn::from((account.public().as_ref(), move |message: &[u8]| async {
        let f = wallet
            .sign(message)
            .await
            .map_err(|_| sube::Error::Signing)?
            .as_bytes();

        Ok::<_, Error>(f)
    }));

    signer.sign(&[0; 64][..]).await;

    // let response = SubeBuilder::default()
    //     .with_url("wss://rococo-rpc.polkadot.io/balances/transfer")
    //     .with_body(json!({
    //         "dest": {
    //             "Id": account.public().as_ref()
    //         },
    //         "value": 100000
    //     }))
    //     .with_signer()
    //     .await
    //     .expect("it must work");

    Ok(())
}
