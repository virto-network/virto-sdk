#![feature(error_in_core)]
#![feature(async_closure)]
#![feature(trait_alias)]
#![feature(associated_type_defaults)]

pub mod base;
pub mod std;
pub mod utils;

use async_trait::async_trait;
pub use base::cqrs;
use matrix_sdk::config::SyncSettings;
pub use matrix_sdk::Client;

use serde::Serialize;
use url::Url;

#[derive(Debug)]
pub enum AuthError {
    WrongCredentials,
    MissingHomeServer,
    Cancelled,
    Unknown,
}

#[async_trait::async_trait]
pub trait AuthenticatorBuilder {
    async fn auth(
        &self,
        device_name: String,
        client: Box<Client>,
    ) -> Result<Box<Client>, AuthError>;
}

#[derive(Debug)]
enum SDKError {
    Unknown,
    CantSync,
    AlreadyInited,
}

#[derive(Debug)]
pub struct SDKCore {
    inner: Box<Client>,
    inited: bool,
    next_batch_token: Option<String>,
}

impl SDKCore {
    fn new(inner: Box<Client>) -> Self {
        return Self {
            inner,
            inited: false,
            next_batch_token: None,
        };
    }

    fn client(&self) -> Box<Client> {
        if !self.inited {
            panic!("accesing client before its initialization");
        }
        self.inner.clone()
    }
    
    async fn next_sync(&mut self) -> Result<(), SDKError> {
        let mut settings = SyncSettings::default();

        if let Some(token) = &self.next_batch_token {
            settings = settings.token(token)
        }

        let sync_res = self
            .inner
            .sync_once(settings)
            .await
            .map_err(|_| SDKError::CantSync)?;

        self.next_batch_token = Some(sync_res.next_batch);
        Ok(())
    }

    async fn init(&mut self) -> Result<(), SDKError> {
        if self.inited {
            return Err(SDKError::AlreadyInited);
        }
        self.inited = true;
        self.next_sync();
        Ok(())
    }

    /**
     * Blocking Call
     */
    async fn start(&self) -> Result<(), SDKError> {
        let mut settings = SyncSettings::default();

        if let Some(token) = &self.next_batch_token {
            settings = settings.token(token)
        }

        self.inner
            .sync(settings)
            .await
            .map_err(|_| SDKError::CantSync)?;
        Ok(())
    }
}

pub struct SDKBuilder {
    homeserver_url: Option<String>,
    credentials: Option<(String, String)>,
    custom_authenticator: Option<Box<dyn AuthenticatorBuilder + Sync + Send>>,
    client: Option<Box<Client>>,
    device_name: Option<String>,
}

impl SDKBuilder {
    fn new() -> Self {
        Self {
            homeserver_url: None,
            client: None,
            custom_authenticator: None,
            credentials: None,
            device_name: None,
        }
    }

    fn with_homeserver(self, homeserver_url: impl Into<String>) -> Self {
        Self {
            homeserver_url: Some(homeserver_url.into()),
            ..self
        }
    }

    fn with_device_name(self, id: impl Into<String>) -> Self {
        Self {
            device_name: Some(id.into()),
            ..self
        }
    }

    fn with_authenticator(
        self,
        custom_authenticator: Box<dyn AuthenticatorBuilder + Sync + Send>,
    ) -> Self {
        Self {
            custom_authenticator: Some(custom_authenticator),
            ..self
        }
    }

    fn with_credentials(self, username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            credentials: Some((username.into(), password.into())),
            ..self
        }
    }

    async fn build_and_login(self) -> Result<SDKCore, AuthError> {
        let Self {
            custom_authenticator,
            credentials,
            homeserver_url,
            ..
        } = self;

        let homeserver_url = homeserver_url.ok_or(AuthError::MissingHomeServer)?;
        let mut client = Box::new(
            Client::new(Url::parse(&homeserver_url).expect("Wrong Url"))
                .await
                .map_err(|_| AuthError::Unknown)?,
        );

        if let Some(authenticator) = custom_authenticator {
            client = authenticator
                .auth(
                    self.device_name.unwrap_or("VitoSDK".to_string()),
                    client.clone(),
                )
                .await?;
        } else {
            let (username, password) = credentials.ok_or(AuthError::WrongCredentials)?;
            client
                .matrix_auth()
                .login_username(&username, &password)
                .initial_device_display_name(&self.device_name.unwrap_or("VitoSDK".to_string()))
                .send()
                .await
                .map_err(|_| AuthError::WrongCredentials)?;
        }

        Ok(SDKCore::new(client))
    }
}

#[cfg(test)]
mod client_store_test {
    use crate::base::app::{AppInfo, VAggregate};
    use crate::base::runner::VRunner;
    use crate::std::wallet::aggregate::Wallet;
    use crate::std::wallet::services::{WalletApi, WalletResult, WalletServices};

    use crate::cqrs::mem_store::MemStore;
    use crate::cqrs::store::EventStore;

    use super::base::app::{EventFactoyError, VApp, VEventStoreFactory};
    use super::std::wallet;
    use super::*;

    use libwallet::Message;
    use tokio::test;

    #[derive(Default)]
    struct AuthConnectorMock;

    #[async_trait::async_trait]
    impl AuthenticatorBuilder for AuthConnectorMock {
        async fn auth(&self, _: String, client: Box<Client>) -> Result<Box<Client>, AuthError> {
            Ok(client)
        }
    }

    #[tokio::test]
    async fn login_with_credentials() {
        let connector: AuthConnectorMock = AuthConnectorMock::default();
        let service = SDKBuilder::new()
            .with_homeserver("https://matrix-client.matrix.org")
            .with_credentials("myfooaccoount", "H0l4mund0@123")
            .build_and_login()
            .await
            .expect("error at building");
    }

    #[tokio::test]
    async fn login_with_authenticator() {
        let connector: AuthConnectorMock = AuthConnectorMock::default();
        let service = SDKBuilder::new()
            .with_homeserver("https://matrix-client.matrix.org")
            .with_authenticator(Box::new(AuthConnectorMock::default()))
            .build_and_login()
            .await
            .expect("error at building");
    }

    type WalletAggregate = Wallet<MockWalletService>;

    #[tokio::test]
    async fn test_wallet() {
        let service = WalletAggregate::default();

        let app = VApp::new(
            AppInfo {
                id: "com.virto.wallet".into(),
                name: "Wallet".into(),
                description: "the place where you mangage your creds".into(),
                version: "0.1.1".into(),
                author: "team@virto.network".into(),
                permission: vec![],
            },
            MemStore::<WalletAggregate>::default(),
            vec![],
            WalletServices::new(MockWalletService::default()),
        );
    }

    #[derive(Default)]
    struct MockWalletService;

    #[async_trait]
    impl WalletApi for MockWalletService {
        type SignedPayload = Message;
        async fn sign<'p>(&self, payload: &'p [u8]) -> WalletResult<Self::SignedPayload> {
            Ok(Message::try_from([1u8].as_slice()).expect("hello world"))
        }
    }
}
