//! Network connectivity: WiFi, TLS, WebSocket, chain subscription.
//!
//! All functions push [`UiEvent`]s to a lock-free queue consumed by the UI core.

use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use heapless::spsc::Producer;
use sube::rpc::chainhead::{ChainEvent, ChainHead};

use crate::event::{Status, UiEvent};

/// Connect WiFi with retry loop.
pub async fn wifi_connect(
    controller: &mut esp_radio::wifi::WifiController<'static>,
    tx: &mut Producer<'static, UiEvent, 16>,
) {
    tx.enqueue(UiEvent::Status(Status::Dim("connecting wifi..."))).ok();
    loop {
        match controller.connect_async().await {
            Ok(()) => break,
            Err(e) => {
                log::warn!("WiFi: {:?}, retry in 5s", e);
                tx.enqueue(UiEvent::Status(Status::Error("wifi retry..."))).ok();
                Timer::after(Duration::from_secs(5)).await;
            }
        }
    }
    log::info!("WiFi: connected");
    tx.enqueue(UiEvent::Wifi(true)).ok();
}

/// Wait for DHCP to assign an IP address.
pub async fn wait_for_ip(
    stack: embassy_net::Stack<'static>,
    tx: &mut Producer<'static, UiEvent, 16>,
) {
    tx.enqueue(UiEvent::Status(Status::Dim("getting IP..."))).ok();
    loop {
        if stack.is_config_up() {
            break;
        }
        Timer::after(Duration::from_millis(200)).await;
    }
    log::info!("IP: {:?}", stack.config_v4().map(|c| c.address));
    tx.enqueue(UiEvent::Status(Status::Good("wifi ok"))).ok();
}

/// Connect to kreivo.io and stream block events to the UI.
/// Returns on disconnect so the caller can retry.
pub async fn watch_chain(
    stack: embassy_net::Stack<'static>,
    tx: &mut Producer<'static, UiEvent, 16>,
) -> Result<(), &'static str> {
    tx.enqueue(UiEvent::Status(Status::Dim("connecting..."))).ok();

    let mut rx_buf = [0u8; 4096];
    let mut tx_buf = [0u8; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
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
    tx.enqueue(UiEvent::Status(Status::Dim("TLS..."))).ok();
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
    tx.enqueue(UiEvent::Status(Status::Dim("websocket..."))).ok();
    let ws = sube::rpc::edge::Backend::connect(session, "kreivo.io", "/")
        .await
        .map_err(|e| { log::error!("WS: {e}"); "WS failed" })?;
    log::info!("WebSocket connected");

    // ChainHead subscription
    tx.enqueue(UiEvent::Status(Status::Dim("chain..."))).ok();
    let mut chain = ChainHead::new(ws)
        .await
        .map_err(|e| { log::error!("ChainHead: {e}"); "ChainHead failed" })?;
    log::info!("ChainHead started");

    tx.enqueue(UiEvent::Live(true)).ok();
    tx.enqueue(UiEvent::Status(Status::Good("LIVE"))).ok();

    loop {
        match chain.next_chain_event().await {
            Ok(ChainEvent::NewBlock { hash, .. }) => {
                if let Ok(header) = chain.header(&hash).await {
                    tx.enqueue(UiEvent::Block(header.number as u32)).ok();
                }
            }
            Ok(ChainEvent::Finalized { hashes, .. }) => {
                tx.enqueue(UiEvent::Finalized(hashes.len() as u16)).ok();
            }
            Ok(_) => {}
            Err(e) => {
                log::error!("Chain: {e}");
                return Err("chain disconnected");
            }
        }
    }
}
