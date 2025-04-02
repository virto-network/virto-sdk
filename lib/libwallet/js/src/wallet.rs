use js_sys::Uint8Array;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::from_value;
use wasm_bindgen::prelude::*;

use libwallet::{vault::Simple, Signer, Wallet, Account};

#[derive(Serialize, Deserialize)]
pub enum WalletConstructor {
    Simple(Option<String>),
}

type SimpleVault = Simple<String>;
#[wasm_bindgen(inspectable)]
pub struct JsWallet {
    phrase: String,
    wallet: Wallet<SimpleVault>,
}

#[wasm_bindgen]
impl JsWallet {
    #[wasm_bindgen(constructor)]
    pub fn new(constructor: JsValue) -> Self {
        wasm_logger::init(wasm_logger::Config::default());
        console_error_panic_hook::set_once();
        
        let constructor: WalletConstructor = from_value(constructor).unwrap();

        let (vault, phrase) = match constructor {
            WalletConstructor::Simple(phrase) => match phrase {
                Some(phrase) => {
                    let vault = SimpleVault::from_phrase(&phrase);
                    (vault, String::from(phrase.as_str()))
                }
                _ => {
                    let (vault, mnemonic) = SimpleVault::generate_with_phrase(&mut rand_core::OsRng);
                    (vault, mnemonic.into_phrase())
                }
            },
        };

        JsWallet {
            phrase,
            wallet: Wallet::new(vault),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn phrase(&self) -> String {
        self.phrase.clone()
    }

    #[wasm_bindgen]
    pub async fn unlock(&mut self, id: JsValue, credentials: JsValue) -> Result<(), JsValue> {
        let credentials: <SimpleVault as libwallet::Vault>::Credentials =
            if credentials.is_null() || credentials.is_undefined() {
                None
            } else {
                from_value(credentials).unwrap_or(None)
            };

        let id: <SimpleVault as libwallet::Vault>::Id =
            if id.is_null() || id.is_undefined() {
                None
            } else {
                from_value(id).unwrap_or(None)
            };

        
        self.wallet
            .unlock(id, credentials)
            .await
            .map_err(|e| JsError::new(&e.to_string()))?;

        Ok(())
    }

    #[wasm_bindgen(js_name = getAddress)]
    pub fn get_address(&self) -> Result<JsPublicAddress, JsError> {
        if self.wallet.is_locked() {
            return Err(JsError::new(
                "The wallet is locked. You should unlock it first by using the .unlock() method",
            ));
        }
        let account = self.wallet.default_account().expect("it must be unlocked");

        Ok(JsPublicAddress::new(
            account.public().as_ref().to_vec(),
        ))
    }

    #[wasm_bindgen]
    pub async fn sign(&self, message: &[u8]) -> Result<Box<[u8]>, JsError> {
        if self.wallet.is_locked() {
            return Err(JsError::new(
                "The wallet is locked. You should unlock it first by using the .unlock() method",
            ));
        }

        let sig = self.wallet.sign(message).await.map_err(|e| JsError::new("Failed to sign message".into()))?;
        let is_verified = self
            .wallet
            .default_account()
            .expect("it must be unlocked")
            .verify(&message, &sig.as_ref()).await;

        if !is_verified
        {
            return Err(JsError::new("Message could not be verified"));
        }

        Ok(sig.as_ref().to_vec().into_boxed_slice())
    }

    #[wasm_bindgen]
    pub async fn verify(&self, msg: &[u8], sig: &[u8]) -> Result<bool, JsError> {
        if self.wallet.is_locked() {
            return Err(JsError::new(
                "The wallet is locked. You should unlock it first by using the .unlock() method",
            ));
        }

        Ok(self.wallet.default_account().expect("it must be unlocked").verify(msg, sig).await)
    }
}

#[wasm_bindgen(inspectable)]
pub struct JsPublicAddress {
    repr: Vec<u8>,
}

#[wasm_bindgen]
impl JsPublicAddress {
    #[wasm_bindgen(constructor)]
    pub fn new(repr: Vec<u8>) -> Self {
        Self { repr }
    }

    #[cfg(feature = "hex")]
    #[wasm_bindgen(js_name = toHex)]
    pub fn to_hex(&self) -> JsValue {
        format!("0x{}", hex::encode(&self.repr)).into()
    }

    #[wasm_bindgen(getter)]
    pub fn repr(&self) -> Uint8Array {
        Uint8Array::from(self.repr.as_slice())
    }
}
