use codec::Decode;
use scale_info::TypeInfo;
use std::future::IntoFuture;
use sube::builder::SubeBuilder as Sube;
use sube::{Error, Response, Result};

#[derive(Decode, TypeInfo, Debug)]
pub struct AccountInfo {
    pub nonce: u32,
    pub consumers: u32,
    pub providers: u32,
    pub sufficients: u32,
    pub data: AccountData,
}

#[derive(Decode, TypeInfo, Debug)]
pub struct AccountData {
    pub free: u128,
    pub reserved: u128,
    pub frozen: u128,
    pub flags: u128,
}

fn dummy_signer(_data: &[u8], _signature: &mut [u8; 64]) -> Result<()> {
    Ok(())
}

#[async_std::main]
async fn main() -> Result<()> {
    let response = Sube::<(), _>::default()
            .with_url("wss://rpc.polkadot.io/system/account/public_key")
            .with_signer(dummy_signer)
            .into_future()
            .await?;

    match response {
        Response::Value(value) => {
            let data = value.as_ref(); // Get data as a slice of bytes
            let account_info = AccountInfo::decode(&mut &data[..])
                .map_err(|_| Error::Decode("Failed to decode AccountInfo".into()))?;

            println!("Account info: {:?}", account_info);
        }
        _ => eprintln!("Unexpected response type"),
    }

    Ok(())
}
