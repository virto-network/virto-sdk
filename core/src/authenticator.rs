

use async_trait::async_trait;

pub enum AuthError {
  Unknown,
}

pub type AuthResult<T> = Result<T, AuthError>;

#[async_trait(?Send)]
pub trait Authenticator {
  type Profile<'p>;
  type Credentials<'c>;
  type RegResponse;
  
  async fn register<'m>(&self, profile: &'m Self::Profile<'_>) -> AuthResult<Self::RegResponse>;
  async fn auth<'n>(&self, credentials: &'n Self::Credentials<'_>) -> AuthResult<()>;
}

