use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use parity_scale_codec::{Compact, Encode};
use sube::{
    http::{Backend as HttpBackend, Url},
    meta::Meta,
    meta_ext::Pallet,
    rpc,
    util::to_camel,
    Backend, JsonValue, Sube,
};

pub type SubeClient = Sube<HttpBackend>;
pub type Result<T> = core::result::Result<T, JsError>;

pub fn get_client_and_path(url: String) -> Result<(Sube<HttpBackend>, String)> {
    let url = Url::parse(url.as_str()).expect("Invalid url");
    // i'm cloning here
    let path = url.path().to_string();
    let rpc_uri = match url.scheme() {
        "http" | "https" => {
            let url_str = format!(
                "{}://{}:{}",
                url.scheme(),
                url.host_str().expect("No host"),
                url.port().or_else(|| Some(80)).unwrap()
            );
            Ok(Url::parse(url_str.as_str()).expect("Invalid Url"))
        }
        _ => Err(JsError::new("Invalid URL scheme")),
    }?;

    Ok((Sube::new(HttpBackend::new(rpc_uri)).into(), path))
}

pub fn decode_addresses(value: &JsonValue) -> JsonValue {
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

pub async fn construct_extrinsic_data(
    sube: &SubeClient,
    nonce: u64,
    call_data: &Vec<u8>,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let extra_params = {
        // ImmortalEra
        let era = 0u8;
        let tip: u128 = 0;
        [vec![era], Compact(nonce).encode(), Compact(tip).encode()].concat()
    };

    let additional_params = {
        // Error: Still failing to deserialize the const
        let metadata = sube.metadata().await.expect("Unable to get metdata");

        let data = metadata
            .pallet_by_name("System")
            .expect("System pallet not found on metadata")
            .constants
            .iter()
            .find(|c| c.name == "Version")
            .expect("System_Version constant not found");

        let chain_version = sube
            .decode(data.value.to_vec(), data.ty.id())
            .await
            .expect("chain version not found");

        let chain_version =
            serde_json::to_value(chain_version).expect("unable to cast chain version");

        let spec_version = chain_version
            .get("spec_version")
            .expect("spec_version not found")
            .as_u64()
            .expect("spec_version not a Number") as u32;

        let transaction_version = chain_version
            .get("transaction_version")
            .expect("transaction_version not found")
            .as_u64()
            .expect("transaction_version not a Number") as u32;

        let genesis_block: Vec<u8> = sube
            .block_info(Some(0u32))
            .await
            .expect("genesis block")
            .into();

        [
            spec_version.to_le_bytes().to_vec(),
            transaction_version.to_le_bytes().to_vec(),
            genesis_block.clone(),
            genesis_block.clone(),
        ]
        .concat()
    };

    let signature_payload = [
        call_data.clone(),
        extra_params.clone(),
        additional_params.clone(),
    ]
    .concat();

    Ok((extra_params, signature_payload))
}
