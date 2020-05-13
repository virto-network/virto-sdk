use jsonrpsee::common::Params;

pub struct Chain {
    rpc: jsonrpsee::Client,
}

impl Chain {
    pub async fn connect(url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let rpc = jsonrpsee::ws_client(url).await?;
        Ok(Chain { rpc })
    }

    pub async fn name(&self) -> String {
        self.rpc
            .request("system_chain", Params::None)
            .await
            .unwrap_or("".into())
    }
}
