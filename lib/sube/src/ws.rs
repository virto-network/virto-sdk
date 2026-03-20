use alloc::{collections::BTreeMap, format, string::String, string::ToString, sync::Arc};
use core::sync::atomic::{AtomicU32, Ordering};

use async_tungstenite::tungstenite::client::IntoClientRequest;
use async_tungstenite::tungstenite::Message;
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use no_std_async::Mutex;

#[cfg(not(feature = "js"))]
fn spawn(fut: impl core::future::Future<Output = ()> + Send + 'static) {
    smol::spawn(fut).detach();
}
#[cfg(feature = "js")]
fn spawn(fut: impl core::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(fut);
}

use crate::rpc::{
    IncomingMessage, JsonRpcError, JsonRpcRequest, JsonRpcResponse, Rpc, RpcResult,
    RpcSubscription, Subscription,
};
use crate::Error;

type Id = u32;

pub struct Backend {
    tx: mpsc::UnboundedSender<Message>,
    pending: Arc<Mutex<BTreeMap<Id, oneshot::Sender<JsonRpcResponse>>>>,
    subscriptions: Arc<Mutex<BTreeMap<String, mpsc::UnboundedSender<serde_json::Value>>>>,
    next_id: AtomicU32,
}

impl Rpc for Backend {
    async fn rpc(&self, method: &str, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        log::info!("RPC `{}` (ID={})", method, id);

        let (sender, recv) = oneshot::channel::<JsonRpcResponse>();
        self.pending.lock().await.insert(id, sender);

        let msg = serde_json::to_string(&JsonRpcRequest {
            id,
            jsonrpc: "2.0",
            method,
            params: Some(params),
        })
        .expect("Request is serializable");

        log::debug!("RPC Request {} ...", &msg);

        self.tx
            .unbounded_send(Message::Text(msg.into()))
            .map_err(|err| {
                log::error!("Error sending message: {:?}", err);
                JsonRpcError::new(-32603, "send failed")
            })?;

        let res = recv.await.map_err(|err| {
            log::error!("Error receiving message: {:?}", err);
            JsonRpcError::new(-32603, "recv failed")
        })?;

        res.into_result()
    }
}

impl RpcSubscription for Backend {
    async fn subscribe(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> RpcResult<(String, Subscription)> {
        let sub_id: String = serde_json::from_value(self.rpc(method, params).await?)
            .map_err(|e| JsonRpcError::new(-32603, &format!("bad sub id: {e}")))?;

        let (tx, rx) = mpsc::unbounded();
        self.subscriptions.lock().await.insert(sub_id.clone(), tx);

        Ok((sub_id, Subscription { rx }))
    }

    async fn unsubscribe(&self, method: &str, sub_id: &str) -> RpcResult<()> {
        self.subscriptions.lock().await.remove(sub_id);
        let _ = self.rpc(method, serde_json::json!([sub_id])).await?;
        Ok(())
    }
}

impl Backend {
    pub async fn new_ws2(url: &str) -> core::result::Result<Self, Error> {
        log::trace!("WS connecting to {}", url);

        let mut request = url
            .into_client_request()
            .map_err(|e| Error::Node(format!("websocket request: {e}")))?;
        request
            .headers_mut()
            .insert("User-Agent", "sube/1.0".parse().expect("valid header"));

        let host = request.uri().host().unwrap_or("localhost").to_string();
        let scheme = request.uri().scheme_str().unwrap_or("ws");
        let port = request
            .uri()
            .port_u16()
            .unwrap_or(if scheme == "wss" || scheme == "https" {
                443
            } else {
                80
            });

        let tcp = smol::net::TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|e| Error::Node(format!("tcp connect: {e}")))?;

        let (ws_stream, _response) = async_tungstenite::async_tls::client_async_tls(request, tcp)
            .await
            .map_err(|e| Error::Node(format!("websocket connect: {e}")))?;

        let (mut sink, mut stream) = ws_stream.split();

        let (tx, mut rx) = mpsc::unbounded::<Message>();
        let pending: Arc<Mutex<BTreeMap<Id, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let subscriptions: Arc<Mutex<BTreeMap<String, mpsc::UnboundedSender<serde_json::Value>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));

        // Outgoing messages: forward from channel to websocket sink
        spawn(async move {
            while let Some(msg) = rx.next().await {
                if let Err(e) = sink.send(msg).await {
                    log::error!("WS send error: {e}");
                    break;
                }
            }
            log::debug!("WS outgoing task stopped");
        });

        // Incoming messages: read from websocket stream, dispatch to pending/subscriptions
        let pending_clone = pending.clone();
        let subs_clone = subscriptions.clone();
        spawn(async move {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(Message::Text(msg)) => {
                        log::trace!("WS message: {}", &msg);
                        match IncomingMessage::parse(&msg) {
                            Some(IncomingMessage::Response(res)) => {
                                if let Some(id) = res.id.as_ref().and_then(|v| v.as_u64()) {
                                    let id = id as Id;
                                    let mut messages = pending_clone.lock().await;
                                    if let Some(channel) = messages.remove(&id) {
                                        if let Err(res) = channel.send(res) {
                                            log::warn!("response error: {:?}", res);
                                        }
                                    }
                                }
                            }
                            Some(IncomingMessage::Notification(notif)) => {
                                let sub_id = &notif.params.subscription;
                                let subs = subs_clone.lock().await;
                                if let Some(sender) = subs.get(sub_id) {
                                    if sender.unbounded_send(notif.params.result).is_err() {
                                        log::warn!("subscription {} receiver dropped", sub_id);
                                    }
                                }
                            }
                            None => {
                                log::warn!("Failed to parse WS message: {}", &msg);
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        log::info!("WS connection closed by server");
                        break;
                    }
                    Ok(_) => {} // Binary, Ping, Pong — handled by tungstenite
                    Err(e) => {
                        log::warn!("WS error: {e}");
                        break;
                    }
                }
            }
            log::debug!("WS incoming task stopped");
        });

        Ok(Backend {
            tx,
            pending,
            subscriptions,
            next_id: AtomicU32::new(1),
        })
    }
}
