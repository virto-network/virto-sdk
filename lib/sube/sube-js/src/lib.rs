mod util;

use core::convert::TryInto;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen;
use sube::{
    sube, Error as SubeError, ExtrinsicBody, JsonValue, Response, SubeBuilder
};
use util::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
extern crate console_error_panic_hook;
use wasm_logger;

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



#[derive(Serialize, Deserialize, Debug)]
struct ExtrinsicBodyWithFrom {
    from: Vec<u8>,
    call: ExtrinsicBody<JsonValue>,
}

#[wasm_bindgen]
pub async fn sube_js(
    url: &str,
    params: JsValue,
    signer: Option<js_sys::Function>,
) -> Result<JsValue> {
    
    wasm_logger::init(wasm_logger::Config::default());
    console_error_panic_hook::set_once();

    log::info!("sube_js: {:?}", params);

    if params.is_undefined() {  
        let response = sube!(url)
            .await
            .map_err(|e| JsError::new(&format!("Error querying: {:?}", &e.to_string())))?;

        let value = match response {
            v @ Response::Value(_) | v @ Response::Meta(_) | v @ Response::Registry(_) => {
                let value = serde_wasm_bindgen::to_value(&v)
                    .map_err(|_| JsError::new("failed to serialize response"))?;
                Ok(value)
            }
            _ => Err(JsError::new("Nonve value at query")),
        }?;

        return Ok(value);
    }

    let mut extrinsic_value: ExtrinsicBodyWithFrom = serde_wasm_bindgen::from_value(params)?;

    extrinsic_value.call.body = decode_addresses(&extrinsic_value.call.body);

    log::info!("new extrinsic_value: {:?}", extrinsic_value);

    let signer = sube::SignerFn::from((
        extrinsic_value.from,
        |message: &[u8]| {
            let message = message.to_vec();
            let signer = signer
                        .clone();

            async move {
                    let promise = signer
                        .ok_or(SubeError::BadInput)?
                        .call1(
                            &JsValue::null(), 
                            &JsValue::from(js_sys::Uint8Array::from(message.to_vec().as_ref())),
                        )
                        .map_err(|_| SubeError::Signing)?;

                    let response = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(promise))
                        .await
                        .map_err(|_| SubeError::Signing)?;

                    let vec: Vec<u8> = serde_wasm_bindgen::from_value(response)
                        .map_err(|_| SubeError::Encode("Unknown value to decode".into()))?;

                    let buffer: [u8; 64] = vec.try_into().expect("slice with incorrect length");

                    Ok(buffer)
            }
        },
    ));

    let value = SubeBuilder::default()
        .with_url(url)
        .with_body(extrinsic_value.call.body)
        .with_signer(signer)
        .await
        .map_err(|e| JsError::new(&format!("Error trying: {:?}", e.to_string())))?;


    match value {
        Response::Void => Ok(JsValue::null()),
        _ => Err(JsError::new("Unknown Response")),
    }
}
