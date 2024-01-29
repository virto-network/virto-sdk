use wasm_bindgen::prelude::*;
use virto_sdk::{ authenticator::AuthError, signer::{self, SignerError}, AuthResult, Authenticator, Signer, SignerResult };
use async_trait::async_trait;
use web_sys::{ console, window, AuthenticatorResponse, CredentialCreationOptions, CredentialRequestOptions, Navigator, PublicKeyCredential, PublicKeyCredentialCreationOptions, PublicKeyCredentialRequestOptions, PublicKeyCredentialRpEntity, PublicKeyCredentialUserEntity, UserVerificationRequirement };
use serde::{ Serialize, Deserialize };
use serde_json;
use js_sys::{ Uint8Array, Array };
use serde_wasm_bindgen;
use core::fmt;
use wasm_bindgen_futures::JsFuture;

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



#[derive(Serialize, Deserialize)]
pub struct KeyType {
  alg: i32,
  #[serde(rename = "type")]
  type_credential: String
}


#[async_trait(?Send)]
impl Authenticator for WebAuthN {
  type Credentials<'c> = Credentials<'c>;
  type Profile<'p> = Profile<'p>;
  type RegResponse = PublicKeyCredential;

  async fn register<'m>(&self, profile: &'m Profile<'_>) -> AuthResult<Self::RegResponse> {
    let window = window().expect("expect window to exist");
    let navigator = window.navigator();
    let credential_manager = navigator.credentials();
    let mut credential_options = CredentialCreationOptions::new();


    let mut slice: [u8; 32] = [0; 32];

    let crypto = window.crypto().expect("crypto to be enabled");
    crypto.get_random_values_with_u8_array(&mut slice);

    let challenge = unsafe { Uint8Array::new(&Uint8Array::view(slice.as_slice())) };
    let id = unsafe { Uint8Array::new(&Uint8Array::view(slice.as_slice())) };
    
    let creds = vec![
      KeyType { alg: -257, type_credential: "public-key".to_string()  },
      KeyType { alg: -7, type_credential: "public-key".to_string()  }
    ];

    // let pub_key = P
    let pub_key = PublicKeyCredentialCreationOptions::new(
      &challenge,
      &serde_wasm_bindgen::to_value(&creds).unwrap(),
      &PublicKeyCredentialRpEntity::new(&self.rp_id),
      &PublicKeyCredentialUserEntity::new(profile.username, profile.display_name, &id)
    );
    
    credential_options.public_key(&pub_key);

    let promise = credential_manager.create_with_options(&credential_options).map_err(|e| AuthError::Unknown)?;
    let public_key_cred: PublicKeyCredential = JsFuture::from(promise).await.map_err(|_| AuthError::Unknown)?.into();
    
    Ok(public_key_cred)
  }

  async fn auth<'m>(&self, credentials: &'m Credentials<'_>) -> AuthResult<()> {
    Ok(())
  }
}

#[async_trait(?Send)]
impl Signer for WebAuthN {
  type SignedPayload = PublicKeyCredential;
  
  async fn sign<'p>(&self, payload: &'p [u8]) -> SignerResult<Self::SignedPayload> {
    let window = window().expect("expect window to exist");
    let navigator = window.navigator();
    let credential_manager = navigator.credentials();
    let mut credential_request = CredentialRequestOptions::new();
    let challenge = unsafe { Uint8Array::new(&Uint8Array::view(payload)) };
    let mut pub_req = PublicKeyCredentialRequestOptions::new(&challenge);
    pub_req.user_verification(UserVerificationRequirement::Required);
    pub_req.rp_id(&self.rp_id);
    credential_request.public_key(&pub_req);

    let promise = credential_manager.get_with_options(&credential_request).map_err(|e| SignerError::Unknown)?;
    let public_key_cred: PublicKeyCredential = JsFuture::from(promise).await.map_err(|_| SignerError::Unknown)?.into();
    Ok(public_key_cred)
  }
}



#[wasm_bindgen]
impl WebAuthN{
  #[wasm_bindgen(constructor)]
  pub fn  new(rp_id: String) -> WebAuthN {
    WebAuthN {
      rp_id
    }
  }
  #[wasm_bindgen(method)]
  pub async fn register_user(&self, username: String) -> Result<JsValue, JsValue> {
    let pub_credential = self.register(&Profile {
      username: &username,
      display_name: &username
    })
    .await
    .map_err(|_| JsError::new("Error Occurrend"))?;

    Ok(pub_credential.into())
  }

  #[wasm_bindgen(method)]
  pub async fn sign_payload(&self, payload: Uint8Array) -> Result<JsValue, JsValue>{
    let pub_credential = self.sign(&payload.to_vec())
    .await
    .map_err(|_| JsError::new("Error Occurrend"))?;

    Ok(pub_credential.into())
  }
}