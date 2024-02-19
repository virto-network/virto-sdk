#![feature(associated_type_defaults)]

pub mod authenticator;
pub mod signer;
pub mod transport;
pub use authenticator::{AuthResult, Authenticator};
pub use transport::Transport;

use core::fmt::Debug;
pub use signer::{Signer, SignerResult};

use serde::Serialize;
use serde_json::json;
use sube::{builder::QueryBuilder, Response};

pub enum VirtoError {
    Unknown,
}

pub type SDKResult<T> = Result<T, VirtoError>;

pub struct VirtoSDKOps {
    device_name: String,
    matrix_host_name: String,
}

pub struct VirtoSDK<A: Authenticator, S: Signer, T: Transport> {
    options: VirtoSDKOps,
    signer: S,
    authenticator: A,
    transport: T,
}

#[derive(Serialize)]
pub struct TxCall<Body> {
    nonce: Option<u64>,
    body: Body,
}

impl<A, S, T> VirtoSDK<A, S, T>
where
    A: Authenticator,
    S: Signer,
    T: Transport,
{
    fn new(options: VirtoSDKOps, authenticator: A, signer: S, transport: T) -> Self {
        VirtoSDK {
            options,
            transport,
            authenticator,
            signer,
        }
    }

    async fn register_device(&self) -> SDKResult<()> {
        // let register_res = self
        //     .authenticator
        //     .register(self.profile)
        //     .await
        //     .map_err(|_| VirtoError::Unknown)?;

        Ok(())
    }

    async fn auth<'m>(&self, c: A::Credentials<'m>) -> SDKResult<()> {
        let f = self
            .authenticator
            .auth(&c)
            .await
            .map_err(|_| VirtoError::Unknown)?;

        Ok(())
    }

    async fn query<'n>(path: &'n str) -> SDKResult<Response<'n>> {
        QueryBuilder::default()
            .with_url(path)
            .await
            .map_err(|_| VirtoError::Unknown)
    }

    async fn tx<'s, Body: Debug + Serialize>(
        &self,
        url: &str,
        body: TxCall<&'s Body>,
    ) -> SDKResult<()> {
        let json_value = json!({
          "url": url,
          "body": body,
        });

        let signed_payload = self
            .signer
            .sign(json_value.as_str().expect("Unknown JSON Value").as_bytes())
            .await
            .map_err(|_| VirtoError::Unknown)?;

        Ok(())
    }
}
