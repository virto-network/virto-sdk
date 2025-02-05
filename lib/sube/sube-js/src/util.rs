use sube::JsonValue;
use wasm_bindgen::prelude::*;

pub type Result<T> = core::result::Result<T, JsError>;
#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

pub fn decode_addresses(value: &JsonValue) -> JsonValue {
    log(format!("{:?}", value).as_str());
    match value {
        JsonValue::Object(o) => o.iter().map(|(k, v)| (k, decode_addresses(v))).collect(),
        JsonValue::String(s) => {
            if s.starts_with("0x") {
                let input = s.as_str();
                let decoded = hex::decode(&input[2..])
                    .expect("strings that start with 0x should be hex encoded")
                    .into_iter()
                    .map(|b| serde_json::json!(b))
                    .collect::<Vec<JsonValue>>();
                JsonValue::Array(decoded)
            } else {
                JsonValue::String(s.clone())
            }
        }
        _ => value.clone(),
    }
}
