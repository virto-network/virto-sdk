use alloc::{collections::BTreeMap, sync::Arc};

use ewebsock::{WsEvent, WsMessage as Message, WsReceiver as Rx, WsSender as Tx};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt as _;
use no_std_async::Mutex;
// use futures_util::StreamExt;
use jsonrpc::{
    error::{result_to_response, standard_error, StandardError},
    serde_json,
};
use log::info;
use serde::Deserialize;

#[cfg(not(feature = "js"))]
use async_std::task::spawn;
#[cfg(feature = "js")]
use async_std::task::spawn_local as spawn;

use crate::{
    rpc::{self, Rpc, RpcResult},
    Error,
};

const MAX_BUFFER: usize = usize::MAX >> 3;

type Id = u32;

pub struct Backend {
    tx: Mutex<mpsc::Sender<Message>>,
    ws_sender: Arc<Mutex<Tx>>,
    messages: Arc<Mutex<BTreeMap<Id, oneshot::Sender<rpc::Response>>>>,
}
unsafe impl Send for Backend {}
unsafe impl Sync for Backend {}

impl Rpc for Backend {
    async fn rpc<T>(&self, method: &str, params: &[&str]) -> RpcResult<T>
    where
        T: for<'a> Deserialize<'a>,
    {
        let id = self.next_id().await;
        info!("RPC `{}` (ID={})", method, id);

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

        log::debug!("RPC Request {} ...", &msg);

        self.tx
            .lock()
            .await
            .try_send(Message::Text(msg))
            .map_err(|err| {
                log::error!("Error tx lock message: {:?}", err);
                standard_error(StandardError::InternalError, None)
            })?;

        log::info!("sent CMD");
        // wait for the matching response to arrive
        let res = recv
            .await
            .map_err(|err| {
                log::error!("Error receiving message: {:?}", err);
                standard_error(StandardError::InternalError, None)
            })?
            .result()?;

        Ok(res)
    }
}

impl Backend {
    async fn next_id(&self) -> Id {
        self.messages.lock().await.keys().last().unwrap_or(&0) + 1
    }

    pub async fn new_ws2<'a, U: Into<&'a str>>(url: U) -> core::result::Result<Self, Error> {
        let url = url.into();
        log::trace!("WS connecting to {}", url);

        let (tx, rx) =
            ewebsock::connect(url, ewebsock::Options::default()).map_err(Error::Platform)?;

        let (sender, recv) = mpsc::channel::<Message>(MAX_BUFFER);

        let backend = Backend {
            tx: Mutex::new(sender),
            ws_sender: Arc::new(Mutex::new(tx)),
            messages: Arc::new(Mutex::new(BTreeMap::new())),
        };

        let recv = Arc::new(Mutex::new(recv));

        backend.process_incoming_messages(rx, backend.ws_sender.clone(), recv.clone());
        Ok(backend)
    }

    fn process_tx_send_messages(tx: Arc<Mutex<Tx>>, recv: Arc<Mutex<mpsc::Receiver<Message>>>) {
        spawn(async move {
            info!("waiting for coommands...");

            while let Some(m) = recv.lock().await.next().await {
                info!("got for coommands...?");
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
        let messages = self.messages.clone();
        spawn(async move {
            while let Some(event) = rx.next().await {
                match event {
                    WsEvent::Message(msg) => {
                        log::trace!("Got WS message {:?}", msg);

                        if let Message::Text(msg) = msg {
                            let res: rpc::Response =
                                serde_json::from_str(&msg).unwrap_or_else(|_| {
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
                                    log::debug!("Answered request id: {}", id);
                                    channel.send(res).expect("receiver waiting");
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
