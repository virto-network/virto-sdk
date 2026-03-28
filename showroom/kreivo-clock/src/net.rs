//! Network connectivity: WiFi, TLS, WebSocket, chain subscription.

use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use sube::rpc::chainhead::{ChainEvent, ChainHead};

/// Connect WiFi with retry loop.
pub async fn wifi_connect(controller: &mut esp_radio::wifi::WifiController<'static>) {
    loop {
        match controller.connect_async().await {
            Ok(()) => break,
            Err(e) => {
                log::warn!("WiFi: {:?}, retry in 5s", e);
                Timer::after(Duration::from_secs(5)).await;
            }
        }
    }
    log::info!("WiFi: connected");
}

/// Wait for DHCP to assign an IP address.
pub async fn wait_for_ip(stack: embassy_net::Stack<'static>) {
    loop {
        if stack.is_config_up() {
            break;
        }
        Timer::after(Duration::from_millis(200)).await;
    }
    log::info!("IP: {:?}", stack.config_v4().map(|c| c.address));
}

/// Block event from the chain.
pub enum BlockEvent {
    NewBlock { number: u64 },
    Finalized { count: usize },
}

/// Connect to kreivo.io and stream block events to the callback.
/// Returns on disconnect so the caller can retry.
pub async fn watch_chain(
    stack: embassy_net::Stack<'static>,
    mut on_event: impl FnMut(BlockEvent),
) -> Result<(), &'static str> {
    // TCP
    let mut rx = [0u8; 4096];
    let mut tx = [0u8; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
    socket.set_timeout(Some(Duration::from_secs(15)));

    let remote = stack
        .dns_query("kreivo.io", embassy_net::dns::DnsQueryType::A)
        .await
        .map_err(|_| "DNS failed")?[0];
    socket
        .connect((remote, 443))
        .await
        .map_err(|_| "TCP failed")?;
    log::info!("TCP connected");

    // TLS (mbedtls, software crypto)
    let mut rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG failed")?;
    let tls_ctx = mbedtls_rs::Tls::new(&mut rng)
        .map_err(|e| { log::error!("TLS init: {:?}", e); "TLS failed" })?;
    let conf = mbedtls_rs::SessionConfig::Client(mbedtls_rs::ClientSessionConfig {
        server_name: Some(c"kreivo.io"),
        auth_mode: mbedtls_rs::AuthMode::None,
        ..mbedtls_rs::ClientSessionConfig::new()
    });
    let mut session = mbedtls_rs::Session::new(tls_ctx.reference(), socket, &conf)
        .map_err(|e| { log::error!("TLS session: {:?}", e); "TLS failed" })?;
    session
        .connect()
        .await
        .map_err(|e| { log::error!("TLS connect: {:?}", e); "TLS failed" })?;
    log::info!("TLS connected");

    // WebSocket
    let ws = sube::rpc::edge::Backend::connect(session, "kreivo.io", "/")
        .await
        .map_err(|e| { log::error!("WS: {e}"); "WS failed" })?;
    log::info!("WebSocket connected");

    // ChainHead subscription
    let mut chain = ChainHead::new(ws)
        .await
        .map_err(|e| { log::error!("ChainHead: {e}"); "ChainHead failed" })?;
    log::info!("ChainHead started");

    on_event(BlockEvent::NewBlock { number: 0 }); // signal "connected"

    // Stream events
    loop {
        match chain.next_chain_event().await {
            Ok(ChainEvent::NewBlock { hash, .. }) => {
                if let Ok(header) = chain.header(&hash).await {
                    on_event(BlockEvent::NewBlock { number: header.number });
                }
            }
            Ok(ChainEvent::Finalized { hashes, .. }) => {
                on_event(BlockEvent::Finalized { count: hashes.len() });
            }
            Ok(_) => {}
            Err(e) => {
                log::error!("Chain: {e}");
                return Err("chain disconnected");
            }
        }
    }
}
