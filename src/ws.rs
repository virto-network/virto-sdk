use crate::prelude::*;
use alloc::{collections::BTreeMap, sync::Arc};
use async_mutex::Mutex;
use async_std::task;
use async_trait::async_trait;
use async_tungstenite::tungstenite::{Error as WsError, Message};
use futures_channel::oneshot;
use futures_util::{
    sink::{Sink, SinkExt},
    stream::SplitSink,
    Stream, StreamExt,
};
use jsonrpc::{
    error::{result_to_response, standard_error, StandardError},
    serde_json,
};

use crate::{
    rpc::{self, Rpc, RpcResult},
    Error,
};

type Id = u8;

pub struct Backend<Tx> {
    tx: Mutex<Tx>,
    messages: Arc<Mutex<BTreeMap<Id, oneshot::Sender<rpc::Response>>>>,
}

#[async_trait]
impl<Tx> Rpc for Backend<Tx>
where
    Tx: Sink<Message, Error = Error> + Unpin + Send,
{
    async fn rpc(&self, method: &str, params: &[&str]) -> RpcResult {
        let id = self.next_id().await;
        log::info!("RPC `{}` (ID={})", method, id);

        // Store a sender that will notify our receiver when a matching message arrives
        let (sender, recv) = oneshot::channel::<rpc::Response>();
        let messages = self.messages.clone();
        messages.lock().await.insert(id, sender);

        // send rpc request
        let msg = serde_json::to_string(&rpc::Request {
            id: id.into(),
            jsonrpc: Some("2.0"),
            method,
            params: &Self::convert_params(params),
        })
        .expect("Request is serializable");
        log::debug!("RPC Request {} ...", &msg[..50]);
        let _ = self.tx.lock().await.send(Message::Text(msg)).await;

        // wait for the matching response to arrive
        let res = recv
            .await
            .map_err(|_| standard_error(StandardError::InternalError, None))?
            .result::<String>()?;
        log::debug!("RPC Response: {}...", &res[..res.len().min(20)]);
        let res = hex::decode(&res[2..])
            .map_err(|_| standard_error(StandardError::InternalError, None))?;
        Ok(res)
    }
}

impl<Tx> Backend<Tx> {
    async fn next_id(&self) -> Id {
        self.messages.lock().await.keys().last().unwrap_or(&0) + 1
    }
}

#[cfg(not(feature = "wss"))]
pub type WS2 = futures_util::sink::SinkErrInto<
    SplitSink<async_tungstenite::WebSocketStream<async_std::net::TcpStream>, Message>,
    Message,
    Error,
>;
#[cfg(feature = "wss")]
pub type WS2 = futures_util::sink::SinkErrInto<
    SplitSink<
        async_tungstenite::WebSocketStream<
            async_tungstenite::stream::Stream<
                async_std::net::TcpStream,
                async_tls::client::TlsStream<async_std::net::TcpStream>,
            >,
        >,
        Message,
    >,
    Message,
    Error,
>;

impl Backend<WS2> {
    pub async fn new_ws2(url: &str) -> core::result::Result<Self, WsError> {
        log::trace!("WS connecting to {}", url);
        let (stream, _) = async_tungstenite::async_std::connect_async(url).await?;
        let (tx, rx) = stream.split();

        let backend = Backend {
            tx: Mutex::new(tx.sink_err_into()),
            messages: Arc::new(Mutex::new(BTreeMap::new())),
        };

        backend.process_incoming_messages(rx);

        Ok(backend)
    }

    fn process_incoming_messages<Rx>(&self, mut rx: Rx)
    where
        Rx: Stream<Item = core::result::Result<Message, WsError>> + Unpin + Send + 'static,
    {
        let messages = self.messages.clone();

        task::spawn(async move {
            while let Some(msg) = rx.next().await {
                match msg {
                    Ok(msg) => {
                        log::trace!("Got WS message {}", msg);
                        if let Ok(msg) = msg.to_text() {
                            let res: rpc::Response =
                                serde_json::from_str(msg).unwrap_or_else(|_| {
                                    result_to_response(
                                        Err(standard_error(StandardError::ParseError, None)),
                                        ().into(),
                                    )
                                });
                            if res.id.is_u64() {
                                let id = res.id.as_u64().unwrap() as Id;
                                log::trace!("Answering request {}", id);
                                let mut messages = messages.lock().await;
                                if let Some(channel) = messages.remove(&id) {
                                    channel.send(res).expect("receiver waiting");
                                    log::debug!("Answered request id: {}", id);
                                }
                            }
                        }
                    }
                    Err(err) => {
                        log::warn!("WS Error: {}", err);
                    }
                }
            }
            log::warn!("WS connection closed");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, Sube};
    use once_cell::sync::OnceCell;

    type WSBackend = Backend<WS2>;
    const CHAIN_URL: &str = "ws://localhost:24680";
    static NODE: OnceCell<Sube<WSBackend>> = OnceCell::new();

    #[async_std::test]
    async fn get_simple_storage_value() -> core::result::Result<(), Error> {
        let node = get_node().await?;

        let latest_block: u32 = node.query("system/number").await?;
        assert!(latest_block > 0, "Block {} greater than 0", latest_block);

        Ok(())
    }

    async fn get_node() -> core::result::Result<&'static Sube<WSBackend>, Error> {
        if let Some(node) = NODE.get() {
            return Ok(node);
        }
        let sube: Sube<_> = Backend::new_ws2(CHAIN_URL)
            .await
            .map_err(|_| Error::ChainUnavailable)?
            .into();
        NODE.set(sube).map_err(|_| Error::ChainUnavailable)?;
        Ok(NODE.get().unwrap())
    }
}
