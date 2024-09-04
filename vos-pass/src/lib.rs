use std::error::Error;

use futures_util::{Sink, Stream, StreamExt};
use matrix_sdk::Client as MxClient;

#[cfg(target_arch = "wasm32")]
mod js_worker;
#[cfg(target_arch = "wasm32")]
use js_worker::get_commands_channel;

#[cfg(not(target_arch = "wasm32"))]
fn get_commands_channel() -> (impl Stream<Item = RawCmd>, impl Sink<u32>) {
    // TODO dummy
    (
        futures_util::stream::once(async {
            RawCmd {
                id: 0,
                cmd: "dummy".into(),
            }
        }),
        futures_util::sink::drain(),
    )
}

pub async fn run() -> Result<(), Box<dyn Error>> {
    let (cmds, res) = get_commands_channel();
    let client = Client::new(cmds);
    client.answer_commands(res).await;
    Ok(())
}

struct RawCmd {
    id: u32,
    cmd: String,
}

struct Client<Cmds> {
    mx: Option<MxClient>,
    cmd_stream: Option<Cmds>,
}

impl<Cmds: Stream<Item = RawCmd>> Client<Cmds> {
    pub fn new(cmds: Cmds) -> Self {
        Self {
            mx: None,
            cmd_stream: Some(cmds),
        }
    }

    pub async fn answer_commands(mut self, responder: impl Sink<u32>) {
        let mut cmds = Box::pin(self.cmd_stream.take().unwrap());
        // make sure we are connected or first command is to connect
        if self.mx.is_none() {
            while let Some(RawCmd { id, cmd }) = cmds.next().await {
                if let Some(_args) = cmd.strip_prefix("auth ") {
                    self.connect("todo!").await;
                } else {
                    log::debug!("ignored cmd `{id}`");
                }
            }
        }
        let _mx = self.mx.expect("matrix connected");
        if cmds
            .then(|RawCmd { id, cmd: _ }| async move { Ok(id) })
            .forward(responder)
            .await
            .is_err()
        {
            log::warn!("command stream closed");
        }
    }

    async fn connect(&mut self, _user: &str) {
        self.mx
            .replace(MxClient::new("".try_into().unwrap()).await.expect("matrix"));
    }
}
