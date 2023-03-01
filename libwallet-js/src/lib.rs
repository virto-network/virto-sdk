use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::from_value;
use wasm_bindgen::prelude::*;

use libwallet::{vault::Simple, Wallet};

// #[wasm_bindgen(js_name = Account)]
// pub struct AccountWrapper(Account);

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn og(s: &str);
}

#[derive(Serialize, Deserialize)]
pub enum WalletConstructor {
    Simple(Option<String>),
}

#[wasm_bindgen(js_name = Wallet)]
pub struct JsWallet {
    wallet: Wallet<Simple>,
}

#[wasm_bindgen(js_class = Wallet)]
impl JsWallet {
    #[wasm_bindgen(constructor)]
    pub fn new(constructor: JsValue) -> Self {
        let constructor: WalletConstructor = from_value(constructor).unwrap();

        let vault = match constructor {
            WalletConstructor::Simple(phrase) => match phrase {
                Some(phrase) => Simple::from_phrase(phrase),
                _ => Simple::generate_with_phrase(&mut rand_core::OsRng).0,
            },
        };

        JsWallet {
            wallet: Wallet::new(vault),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn address(&self) -> Result<Box<[u8]>, JsError> {
        if self.wallet.is_locked() {
            return Err(JsError::new("The wallet is locked. You should unlock it first by using the .unlock() method"));
        }

        Ok(self.wallet
            .default_account()
            .public()
            .as_ref()
            .to_vec()
            .into_boxed_slice())
    }

    #[wasm_bindgen]
    pub async fn unlock(&mut self, credentials: JsValue) -> Result<(), JsValue> {
        let credentials: <Simple as libwallet::Vault>::Credentials =
            if credentials.is_null() || credentials.is_undefined() {
                ()
            } else {
                from_value(credentials).unwrap_or(())
            };

        self.wallet
            .unlock(credentials)
            .await
            .map_err(|e| JsError::new(&e.to_string()))?;

        Ok(())
    }
}
