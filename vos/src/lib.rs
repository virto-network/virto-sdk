use core::fmt;
use matrix_sdk::{
    config::SyncSettings,
    ruma::{events::room::message::SyncRoomMessageEvent, UserId},
    Client,
};
use std::path::PathBuf;

mod command_engine;

pub struct Cfg<'a> {
    pub uid: &'a str,
    pub pwd: &'a str,
    pub home: PathBuf,
}

pub async fn start(cfg: Cfg<'_>) -> Result<(), Error> {
    let Cfg { uid, pwd, home } = cfg;

    let db_path = home.join("matrix");
    let bot = start_matrix_client(uid, pwd, db_path.to_str().expect("unicode")).await?;
    bot.sync(SyncSettings::default())
        .await
        .map_err(|_| Error::MatrixSync)?;
    Ok(())
}

#[derive(Debug)]
pub enum Error {
    MatrixSetup(&'static str),
    MatrixSync,
}
impl std::error::Error for Error {}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MatrixSetup(e) => write!(f, "matrix setup error: {e}"),
            Error::MatrixSync => write!(f, "matrix sync error"),
        }
    }
}

async fn start_matrix_client(uid: &str, pwd: &str, db_path: &str) -> Result<Client, Error> {
    let uid = <&UserId>::try_from(uid).map_err(|_| Error::MatrixSetup("bad bot id"))?;
    let client = Client::builder()
        .server_name(uid.server_name())
        .sqlite_store(db_path, Some(pwd))
        .build()
        .await
        .map_err(|_| Error::MatrixSetup("creating client"))?;
    client
        .matrix_auth()
        .login_username(uid, pwd)
        .device_id("VOS-FIDO-1")
        .send()
        .await
        .map_err(|_| Error::MatrixSetup("credentials"))?;

    client.add_event_handler(|ev: SyncRoomMessageEvent| async move {
        println!("Received a message {:?}", ev);
    });

    Ok(client)
}
