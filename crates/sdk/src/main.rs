fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    smol::block_on(async {
        let chain_url =
            std::env::var("CHAIN_URL").unwrap_or_else(|_| "wss://kreivo.io".into());
        let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:4000".into());

        log::info!("connecting to {chain_url}...");
        let mut chain = sube::Sube::connect(&chain_url).await?;
        log::info!(
            "connected — {} pallets loaded",
            chain.metadata().pallets.len()
        );

        virto_sdk::server::run(&addr, &mut chain).await?;
        Ok(())
    })
}
