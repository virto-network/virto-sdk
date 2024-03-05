pub mod prelude {
    pub use async_trait::async_trait;
    pub use core::marker::PhantomData;
    pub use futures::future;
    pub use matrix_sdk::{config::SyncSettings as MatrixSyncSettings, Client as MatrixClient};

    pub use serde::{de::DeserializeOwned, Deserialize, Serialize};
    pub use serde_json::{ Value, Map };
    pub use url::Url;
    // special type;
    pub use std::collections::HashMap;
    pub use core::fmt::Debug;
}
