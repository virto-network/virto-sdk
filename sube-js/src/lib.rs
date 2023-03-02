mod utils;

use serde::{Deserialize, Serialize};
use serde_json;
use serde_wasm_bindgen;
use std::{fmt, string, collections::HashMap};
use sube::{
    http::{Backend as HttpBackend, Url},
    meta::Meta,
    meta_ext::Pallet,
    rpc,
    util::to_camel,
    Backend
};

// use sp_core::{crypto::Ss58Codec, hexdisplay::AsBytesRef};
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;
use utils::*;

#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[derive(Serialize, Deserialize)]
struct SubeOptions {
    pub nonce: Option<u64>,
    pub signer: Closure<dyn Fn()>,
    pub params: HashMap<String, JsValue>
}

#[wasm_bindgen]
pub async fn sube_js(url: &str, params: JsValue, signer: js_sys::Function) -> Result<JsValue> {
    let (client, path) = get_client_and_path(url.to_string())?;

    if params.is_undefined() {
        let value = client.query(path.as_str()).await?;
        Ok(serde_wasm_bindgen::to_value(&value).expect("Json parsing error"))
    } else {        
        let meta = client.metadata().await?;

        let mut path = path.trim_matches('/').split('/');
        let pallet = path.next().map(to_camel).expect("Unknown path");
        let call = path.next().expect("Unknown item");

        let (ty, index) = {
            let pallet = meta.pallet_by_name(&pallet).expect("pallet does not exist");

            (
                pallet
                    .get_calls()
                    .expect("pallet does not have calls")
                    .ty
                    .id(),
                pallet.index,
            )
        };

        let params = serde_wasm_bindgen::from_value(params)?;
        let params = decode_addresses(&params);
        let value = serde_json::json!({ call: params });
        log(format!("Payload Params({}, {}) {:?} ", index, ty, value).as_str());

        let value = [vec![index], client.encode(value, ty).await?].concat();
        let call_data = client.encode(value, ty).await?;

        let mut encoded_call = vec![index];
        encoded_call.extend(call_data);

        log(format!("encoded: {:?}", encoded_call).as_str());
        Ok(JsValue::UNDEFINED)
    }
}
