use crate::prelude::*;
use core::fmt;

#[derive(Serialize, Deserialize)]
pub struct KeyType {
    pub alg: i32,
    #[serde(rename = "type")]
    pub type_credential: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SigningResponse {
    #[serde(rename = "authenticatorData")]
    #[serde(deserialize_with = "from_bytes", serialize_with = "as_base64")]
    authenticatorr_data: Vec<u8>,
    // clientDataJSON
    #[serde(rename = "clientDataJSON")]
    #[serde(deserialize_with = "from_bytes", serialize_with = "as_base64")]
    client_data_json: Vec<u8>,

    #[serde(deserialize_with = "from_bytes", serialize_with = "as_base64")]
    signature: Vec<u8>,
    // userHandle
    #[serde(rename = "userHandle")]
    #[serde(deserialize_with = "from_bytes", serialize_with = "as_base64")]
    user_handle: Vec<u8>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct RegResponse {
    #[serde(rename = "attestationObject")]
    #[serde(deserialize_with = "from_bytes", serialize_with = "as_base64")]
    attestation_object: Vec<u8>,

    #[serde(rename = "clientDataJSON")]
    #[serde(deserialize_with = "from_bytes", serialize_with = "as_base64")]
    client_data_json: Vec<u8>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct WebAuthnResult<T> {
    #[serde(rename = "authenticatorAttachment")]
    authenticator_attachment: String,
    id: String,
    #[serde(rename = "rawId")]
    #[serde(deserialize_with = "from_bytes", serialize_with = "as_base64")]
    raw_id: Vec<u8>,
    response: T,
}

fn as_base64<S>(key: &[u8], se: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    se.serialize_str(&base64::encode(key))
}

fn from_bytes<'de, D>(des: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    struct VecVisitor;

    impl<'de> Visitor<'de> for VecVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a byte array")
        }

        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            utils::SeqIter::new(seq).collect::<Result<_, _>>()
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            Ok(v.to_vec())
        }

        fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            Ok(v)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            Ok(v.as_bytes().to_vec())
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            Ok(v.into_bytes())
        }
    }

    des.deserialize_byte_buf(VecVisitor)
}

impl<T> From<WebAuthnResult<T>> for JsValue
where
    T: Serialize,
{
    fn from(value: WebAuthnResult<T>) -> Self {
        serde_wasm_bindgen::to_value(&value).expect("Error cann't translate to JsValue")
    }
}
