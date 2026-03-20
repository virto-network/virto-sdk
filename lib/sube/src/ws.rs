use alloc::{collections::BTreeMap, string::String, sync::Arc};
use core::sync::atomic::{AtomicU32, Ordering};

use ewebsock::{WsEvent, WsMessage as Message, WsReceiver as Rx, WsSender as Tx};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt as _;
use no_std_async::Mutex;

#[cfg(not(feature = "js"))]
use async_std::task::spawn;
#[cfg(feature = "js")]
use async_std::task::spawn_local as spawn;

use crate::rpc::{
    IncomingMessage, JsonRpcError, JsonRpcRequest, JsonRpcResponse, Rpc, RpcResult,
    RpcSubscription, Subscription,
};
use crate::Error;

const MAX_BUFFER: usize = usize::MAX >> 3;

type Id = u32;

pub struct Backend {
    tx: Mutex<mpsc::Sender<Message>>,
    ws_sender: Arc<Mutex<Tx>>,
    pending: Arc<Mutex<BTreeMap<Id, oneshot::Sender<JsonRpcResponse>>>>,
    subscriptions: Arc<Mutex<BTreeMap<String, mpsc::UnboundedSender<serde_json::Value>>>>,
    next_id: AtomicU32,
}
unsafe impl Send for Backend {}
unsafe impl Sync for Backend {}

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
            .lock()
            .await
            .try_send(Message::Text(msg))
            .map_err(|err| {
                log::error!("Error tx lock message: {:?}", err);
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
        // Send the subscribe request — response contains the subscription id
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
    pub async fn new_ws2<'a, U: Into<&'a str>>(url: U) -> core::result::Result<Self, Error> {
        let url = url.into();
        log::trace!("WS connecting to {}", url);

        let (tx, rx) = ewebsock::connect(url, ewebsock::Options::default()).map_err(Error::Node)?;

        let (sender, recv) = mpsc::channel::<Message>(MAX_BUFFER);

        let backend = Backend {
            tx: Mutex::new(sender),
            ws_sender: Arc::new(Mutex::new(tx)),
            pending: Arc::new(Mutex::new(BTreeMap::new())),
            subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
            next_id: AtomicU32::new(1),
        };

        let recv = Arc::new(Mutex::new(recv));

        backend.process_incoming_messages(rx, backend.ws_sender.clone(), recv.clone());
        Ok(backend)
    }

    fn process_tx_send_messages(tx: Arc<Mutex<Tx>>, recv: Arc<Mutex<mpsc::Receiver<Message>>>) {
        spawn(async move {
            log::info!("waiting for commands...");

            while let Some(m) = recv.lock().await.next().await {
                tx.lock().await.send(m);
            }
        });
    }

    fn process_incoming_messages(
        &self,
        mut rx: Rx,
        tx: Arc<Mutex<Tx>>,
        recv: Arc<Mutex<mpsc::Receiver<Message>>>,
    ) {
        let pending = self.pending.clone();
        let subscriptions = self.subscriptions.clone();
        spawn(async move {
            while let Some(event) = rx.next().await {
                match event {
                    WsEvent::Message(msg) => {
                        log::trace!("Got WS message {:?}", msg);

                        if let Message::Text(msg) = msg {
                            match IncomingMessage::parse(&msg) {
                                Some(IncomingMessage::Response(res)) => {
                                    if let Some(id) = res.id.as_ref().and_then(|v| v.as_u64()) {
                                        let id = id as Id;
                                        log::trace!("Answering request {}", id);
                                        let mut messages = pending.lock().await;
                                        if let Some(channel) = messages.remove(&id) {
                                            log::debug!("Answered request id: {}", id);
                                            if let Err(res) = channel.send(res) {
                                                log::warn!("response error: {:?}", res);
                                            }
                                        }
                                    }
                                }
                                Some(IncomingMessage::Notification(notif)) => {
                                    let sub_id = &notif.params.subscription;
                                    let subs = subscriptions.lock().await;
                                    if let Some(sender) = subs.get(sub_id) {
                                        if sender.unbounded_send(notif.params.result).is_err() {
                                            log::warn!("subscription {} receiver dropped", sub_id);
                                        }
                                    } else {
                                        log::debug!(
                                            "notification for unknown subscription {}",
                                            sub_id
                                        );
                                    }
                                }
                                None => {
                                    log::warn!("Failed to parse WS message: {}", &msg);
                                }
                            }
                        }
                    }
                    WsEvent::Error(e) => {
                        log::warn!("WS error {}", &e);
                    }
                    WsEvent::Closed => {
                        log::info!("WS connection closed");
                    }
                    WsEvent::Opened => {
                        log::info!("Processing tx msg");
                        Backend::process_tx_send_messages(tx.clone(), recv.clone());
                        log::trace!("Ws connection opened");
                    }
                }
            }

            log::warn!("WS connection closed");
        });
    }
}
