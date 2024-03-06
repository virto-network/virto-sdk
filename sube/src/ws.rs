use crate::prelude::*;
use alloc::{collections::BTreeMap, sync::Arc};

use async_mutex::Mutex;
use async_std::future::IntoFuture;
use async_std::stream::IntoStream;
use async_std::{channel::Receiver, task};
use async_trait::async_trait;

use futures_channel::mpsc;
use futures_channel::mpsc::TryRecvError;
use futures_channel::oneshot;
use futures_util::stream::Next;

use ewebsock::{Error as WsError, WsMessage as Message, WsReceiver as Rx, WsSender as Tx};

#[cfg(not(feature = "js"))]
use async_std::task::spawn;

#[cfg(feature = "js")]
use async_std::task::spawn_local as spawn;

use futures_util::stream;
use futures_util::{
    sink::{Sink, SinkExt},
    stream::SplitSink,
    Stream, StreamExt,
};

use jsonrpc::{
    error::{result_to_response, standard_error, StandardError},
    serde_json,
};
use log::info;

use crate::{
    rpc::{self, Rpc, RpcResult},
    Error,
};

type Id = u32;

pub struct Backend {
    tx: Mutex<mpsc::Sender<Message>>,
    ws_sender: Arc<Mutex<Tx>>,
    messages: Arc<Mutex<BTreeMap<Id, oneshot::Sender<rpc::Response>>>>,
}
unsafe impl Send for Backend {}
unsafe impl Sync for Backend {}

#[async_trait]
impl Rpc for Backend {
    async fn rpc(&self, method: &str, params: &[&str]) -> RpcResult {
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

        log::debug!("RPC Request {} ...", &msg[..50]);

        // self.tx.lock().await.send
        self.tx
            .lock()
            .await
            .try_send(Message::Text(msg))
            .map_err(|x| Error::Platform("error sending message".into()));

        log::info!("sent comomand");
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

impl Backend {
    async fn next_id(&self) -> Id {
        self.messages.lock().await.keys().last().unwrap_or(&0) + 1
    }

    pub async fn new_ws2<'a, U: Into<&'a str>>(url: U) -> core::result::Result<Self, Error> {
        let url = url.into();
        log::trace!("WS connecting to {}", url);

        // let (receiver, d) =  ewebsock::connect(url).;

        let (tx, rx) =
            ewebsock::connect(url, ewebsock::Options::default()).map_err(|e| Error::Platform(e))?;

        let (mut sender, recv) = mpsc::channel::<Message>(0);

        let backend = Backend {
            tx: Mutex::new(sender),
            ws_sender: Arc::new(Mutex::new(tx)),
            messages: Arc::new(Mutex::new(BTreeMap::new())),
        };

        let recv = Arc::new(Mutex::new(recv));

        backend.process_incoming_messages(rx, backend.ws_sender.clone(), recv.clone());
        Ok(backend)
    }

    fn process_tx_send_messages(
        mut tx: Arc<Mutex<Tx>>,
        mut recv: Arc<Mutex<mpsc::Receiver<Message>>>,
    ) {
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
        mut tx: Arc<Mutex<Tx>>,
        mut recv: Arc<Mutex<mpsc::Receiver<Message>>>,
    ) {
        let messages = self.messages.clone();
        spawn(async move {
            while let Some(event) = rx.next().await {
                info!("gt eveeeeeeen");
                match event {
                    ewebsock::WsEvent::Message(msg) => {
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
                                    channel.send(res).expect("receiver waiting");
                                    log::debug!("Answered request id: {}", id);
                                }
                            }
                        }
                    }
                    ewebsock::WsEvent::Error(e) => {
                        log::warn!("WS error {}", &e);
                    }
                    ewebsock::WsEvent::Closed => {
                        log::warn!("WS connection closed");
                    }
                    ewebsock::WsEvent::Opened => {
                        log::info!("Processing tx msg");
                        Backend::process_tx_send_messages(tx.clone(), recv.clone());
                        log::trace!("Ws connection opened");
                    }
                }
            }
            // }

            log::warn!("WS connection closed");
            // process_incoming_messages(rx);
        });
    }
}
