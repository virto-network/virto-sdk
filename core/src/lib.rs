pub mod signer;
pub mod authenticator;
pub use signer::{ Signer, SignerResult};
pub use authenticator::{Authenticator, AuthResult};
use core::fmt::Debug;

use sube::{ builder::QueryBuilder, Response };
use serde_json::json;
use serde::Serialize;

pub enum VirtoError {
  Unknown
}

pub type SDKResult<T> = Result<T, VirtoError>;

pub trait CallBack<A>: Fn(&A) -> SDKResult<()> {}
impl<T, A> CallBack<A> for T where T: Fn(&A) -> SDKResult<()> {}

pub struct VirtoSDK<'m,
  A: Authenticator, S: Signer,
  RegisterFn: CallBack<A::RegResponse>,
  SignerFn: CallBack<S::SignedPayload>> {
  profile: &'m A::Profile<'m>,
  signer: S,
  authenticator: A,
  send_register_cb: RegisterFn,
  send_signed_cb: SignerFn
}

impl<'a, A: Authenticator, S: Signer,
  RegisterFn: CallBack<A::RegResponse>,
  SignerFn: CallBack<S::SignedPayload>
> VirtoSDK<'a, A, S, RegisterFn, SignerFn> 
where A: 'a {
  fn new(
    profile: &'a A::Profile<'a>,
    signer: S,
    authenticator: A,
    send_register_cb: RegisterFn,
    send_signed_cb: SignerFn
  ) ->  VirtoSDK<'a, A, S, RegisterFn, SignerFn> {
    VirtoSDK {
      profile,
      signer,
      authenticator,
      send_register_cb,
      send_signed_cb,
    }
  }

  async fn register(&self) -> SDKResult<()> {
    let register_res = self.authenticator.register(self.profile)
      .await
      .map_err(|_| VirtoError::Unknown)?;
    (self.send_register_cb)(&register_res);
    Ok(())
  }

  async fn auth<'m>(&self, c: A::Credentials<'m>) -> SDKResult<()> where A: 'm{
    self.authenticator.auth(&c)
      .await
      .map_err(|_| VirtoError::Unknown)?; 
    Ok(())
  }

  async fn query<'n>(path: &'n str) -> SDKResult<Response<'n>> {
    QueryBuilder::default()
      .with_url(&path)
      .await
      .map_err(|_| VirtoError::Unknown)
  }

  async fn tx<'s, Body: Debug + Serialize>(&self, url: &str, body: &'s Body ) -> SDKResult<()> {
    let json_value = json!({
      "url": url,
      "body": body
    });
    
    let signed_payload = self.signer.sign(
      json_value
        .as_str()
        .expect("Unknown JSON Value")
        .as_bytes()
    )
    .await
    .map_err(|_| VirtoError::Unknown)?;

    (self.send_signed_cb)(&signed_payload);
    Ok(())
  }
 }

