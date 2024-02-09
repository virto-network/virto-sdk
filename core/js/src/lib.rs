pub mod utils;
mod models;
use models::*;

mod prelude {
  pub use wasm_bindgen_futures::JsFuture;
  pub use async_trait::async_trait;
  pub use wasm_bindgen::prelude::*;
  pub use web_sys::{ window, CredentialCreationOptions, CredentialRequestOptions, PublicKeyCredentialCreationOptions, PublicKeyCredentialRequestOptions, PublicKeyCredentialRpEntity, PublicKeyCredentialUserEntity };
  pub use serde::{ Serialize, Serializer, Deserialize, Deserializer, de::{ SeqAccess, Visitor, Error as DeError } };
  pub use js_sys::Uint8Array;
  pub use virto_sdk::{ authenticator::AuthError, signer::SignerError, AuthResult, Authenticator, Signer, SignerResult };
  pub use crate::utils;
}

use prelude::*;

pub struct Profile<'a> {
  username: &'a str,
  display_name: &'a str,
}
pub struct Credentials<'a> {
  username: &'a str,
}

#[wasm_bindgen]
pub struct WebAuthN  {
  rp_id: String,
} 

#[async_trait(?Send)]
impl Authenticator for WebAuthN {
  type Credentials<'c> = Credentials<'c>;
  type Profile<'p> = Profile<'p>;
  type RegResponse = WebAuthnResult<RegResponse>;

  async fn register<'m>(&self, profile: &'m Profile<'_>) -> AuthResult<Self::RegResponse> {
    let window = window().expect("expect window to exist");
    let navigator = window.navigator();
    let credential_manager = navigator.credentials();
    let mut credential_options = CredentialCreationOptions::new();

    let mut slice: [u8; 32] = [0; 32];

    let crypto = window.crypto().expect("crypto to be enabled");
    crypto.get_random_values_with_u8_array(&mut slice).expect("Error generating random bytes");

    let challenge = unsafe { Uint8Array::new(&Uint8Array::view(slice.as_slice())) };
    let id = unsafe { Uint8Array::new(&Uint8Array::view(slice.as_slice())) };
    
    let creds = vec![
      KeyType { alg: -257, type_credential: "public-key".to_string()  }, // RSA
      KeyType { alg: -7, type_credential: "public-key".to_string()  } // EDSA
    ];

    // let pub_key = P
    let pub_key = PublicKeyCredentialCreationOptions::new(
      &challenge,
      &serde_wasm_bindgen::to_value(&creds).unwrap(),
      &PublicKeyCredentialRpEntity::new(&self.rp_id),
      &PublicKeyCredentialUserEntity::new(profile.username, profile.display_name, &id)
    );
  
    credential_options.public_key(&pub_key);

    let promise = credential_manager
      .create_with_options(&credential_options)
      .map_err(|_| AuthError::Unknown)?;

    let value = JsFuture::from(promise).await.map_err(|_| AuthError::Unknown)?;

    let public_key_cred = serde_wasm_bindgen::from_value(
      value
    ).expect("Error trying to build webauthn result");

    Ok(public_key_cred)
  }

  async fn auth<'m>(&self, _: &'m Credentials<'_>) -> AuthResult<()> {
    Ok(())
  }
}

#[async_trait(?Send)]
impl Signer for WebAuthN {
  type SignedPayload = WebAuthnResult<SigningResponse>;
  
  async fn sign<'p>(&self, payload: &'p [u8]) -> SignerResult<<Self as Signer>::SignedPayload> {
    let window = window().expect("expect window to exist");
    let navigator = window.navigator();
    let credential_manager = navigator.credentials();
    let mut credential_request = CredentialRequestOptions::new();
    let challenge = unsafe { Uint8Array::new(&Uint8Array::view(payload)) };

    let mut pub_req = PublicKeyCredentialRequestOptions::new(&challenge);
    // pub_req.user_verification(UserVerificationRequirement::Required);
    pub_req.rp_id(&self.rp_id);
    
    js_sys::Reflect::set(
      pub_req.as_ref(),
      &JsValue::from("attestation"),
      &JsValue::from("indirect"),
    ).expect("error setting JsValue");

    credential_request.public_key(&pub_req);

    let promise = credential_manager.get_with_options(&credential_request).map_err(|_| SignerError::Unknown)?;
    let value = JsFuture::from(promise).await.map_err(|_| SignerError::Unknown)?;
    let public_key_cred = serde_wasm_bindgen::from_value(value).expect("hello");
    Ok(public_key_cred)
  }
}


#[wasm_bindgen]
impl WebAuthN{
  #[wasm_bindgen(constructor)]
  pub fn  new(rp_id: String) -> WebAuthN {
    console_error_panic_hook::set_once();
    WebAuthN {
      rp_id
    }
  }
  #[wasm_bindgen()]
  pub async fn register_user(&self, username: String) -> Result<JsValue, JsValue> {
    let pub_credential = self.register(&Profile {
      username: &username,
      display_name: &username
    })
    .await
    .map_err(|_| JsError::new("Error Occurrend"))?;

    Ok(pub_credential.into())
  }

  #[wasm_bindgen()]
  pub async fn sign_payload(&self, payload: Uint8Array) -> Result<JsValue, JsValue>{
    let pub_credential = self.sign(&payload.to_vec())
    .await
    .map_err(|_| JsError::new("Error Occurrend"))?;

    Ok(pub_credential.into())
  }
}