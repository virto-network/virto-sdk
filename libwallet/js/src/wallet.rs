use js_sys::Uint8Array;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::from_value;
use wasm_bindgen::prelude::*;

use libwallet::{vault::Simple, Account, Signer, Wallet};

#[derive(Serialize, Deserialize)]
pub enum WalletConstructor {
    Simple(Option<String>),
}

#[wasm_bindgen(inspectable)]
pub struct JsWallet {
    phrase: String,
    wallet: Wallet<Simple<String>>,
}

#[wasm_bindgen]
impl JsWallet {
    #[wasm_bindgen(constructor)]
    pub fn new(constructor: JsValue) -> Self {
        let constructor: WalletConstructor = from_value(constructor).unwrap();

        let (vault, phrase) = match constructor {
            WalletConstructor::Simple(phrase) => match phrase {
                Some(phrase) => {
                    let vault = Simple::from_phrase(&phrase);
                    (vault, String::from(phrase.as_str()))
                }
                _ => {
                    let (vault, mnemonic) = Simple::generate_with_phrase(&mut rand_core::OsRng);
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
    pub async fn unlock(&mut self, credentials: JsValue) -> Result<(), JsValue> {
        let credentials: <Simple<String> as libwallet::Vault>::Credentials =
            if credentials.is_null() || credentials.is_undefined() {
                None
            } else {
                from_value(credentials).unwrap_or(None)
            };

        self.wallet
            .unlock(None, credentials)
            .await
            .map_err(|e| JsError::new(&e.to_string()))?;

        Ok(())
    }

    #[wasm_bindgen(js_name = getAddress)]
    pub fn get_address(&self) -> Result<JsBytes, JsError> {
        if self.wallet.is_locked() {
            return Err(JsError::new(
                "The wallet is locked. You should unlock it first by using the .unlock() method",
            ));
        }

        Ok(JsBytes::new(
            self.wallet.default_account().unwrap().public().as_ref().to_vec(),
        ))
    }

    #[wasm_bindgen]
    pub async fn sign(&self, message: &[u8]) -> Result<JsBytes, JsError> {
        if self.wallet.is_locked() {
            return Err(JsError::new(
                "The wallet is locked. You should unlock it first by using the .unlock() method",
            ));
        }

        let sig = self.wallet.sign(message).await.map_err(|_| JsError::new("Error while signing"))?;
        let f = sig.as_ref().to_vec();
        Ok(JsBytes::new(f))
    }

    #[wasm_bindgen]
    pub async fn verify(&self, msg: &[u8], sig: &[u8]) -> Result<bool, JsError> {
        if self.wallet.is_locked() {
            return Err(JsError::new(
                "The wallet is locked. You should unlock it first by using the .unlock() method",
            ));
        }

        Ok(self.wallet.default_account().unwrap().verify(msg, sig).await)
    }
}

#[wasm_bindgen(inspectable)]
pub struct JsBytes {
    repr: Vec<u8>,
}

#[wasm_bindgen]
impl JsBytes {
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
