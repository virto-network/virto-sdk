use async_std::net::TcpStream;
use async_trait::async_trait;
use async_tungstenite::{async_std::connect_async, WebSocketStream};

pub struct Backend {
    connection: WebSocketStream<TcpStream>,
}

#[async_trait]
impl crate::Backend for Backend {
    async fn query_raw<K>(&self, key: K) -> crate::Result<Vec<u8>>
    where
        K: std::convert::TryInto<crate::StorageKey, Error = crate::Error> + Send,
    {
        todo!()
    }

    async fn submit<T>(&self, ext: T) -> crate::Result<()>
    where
        T: futures_lite::AsyncRead + Send + Unpin,
    {
        todo!()
    }

    async fn metadata(&self) -> crate::Result<frame_metadata::RuntimeMetadataPrefixed> {
        todo!()
    }
}

impl Backend {
    pub async fn new(url: &str) -> Result<Self, crate::Error> {
        let (connection, _) = connect_async(url)
            .await
            .map_err(|_| crate::Error::ChainUnavailable)?;
        Ok(Backend { connection })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Backend as _, Error, Sube};
    use once_cell::sync::OnceCell;

    const CHAIN_URL: &str = "ws://localhost:24680";
    static NODE: OnceCell<Sube<Backend>> = OnceCell::new();

    #[async_std::test]
    async fn get_simple_storage_value() -> Result<(), Error> {
        let node = get_node().await?;

        let latest_block: u32 = node.query("system/number").await?;
        assert!(latest_block > 0, "Block {} greater than 0", latest_block);

        todo!();
    }

    async fn get_node() -> Result<&'static Sube<Backend>, Error> {
        if let Some(node) = NODE.get() {
            return Ok(node);
        }
        let sube = Backend::new(CHAIN_URL).await?.into();
        NODE.set(sube).map_err(|_| Error::ChainUnavailable)?;
        Ok(NODE.get().unwrap())
    }
}
