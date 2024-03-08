#[derive(Decode, Debug)]
pub struct AccountInfo {
    pub nonce: u32,
    pub consumers: u32,
    pub providers: u32,
    pub sufficients: u32,
    pub data: AccountData,
}

#[derive(Decode, Debug)]
pub struct AccountData {
    pub free: u128,
    pub reserved: u128,
    pub frozen: u128,
    pub flags: u128,
}

async fn get_balance() -> Result<()> {
    let response = QueryBuilder::default()
    .with_url("wss://rococo-rpc.polkadot.io/system/account/0x3c85f79f28628bee75cdb9eddfeae249f813fad95f84120d068fbc990c4b717d")
    .await?;

    match response {
        Response::Value(value) => {
            let data = value.as_ref(); // Obtiene los datos como un slice de bytes
            let account_info = AccountInfo::decode(&mut &data[..])
                .map_err(|_| Error::Decode("Failed to decode AccountInfo".into()))?;

            log::info!("Account info: {:?}", account_info);
        }
        _ => log::error!("Unexpected response type"),
    }

    Ok(())
}
