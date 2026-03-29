//! Network connectivity: WiFi, TLS, WebSocket, chain subscription.
//!
//! All functions push [`UiEvent`]s to a lock-free queue consumed by the UI core.

use alloc::vec;
use alloc::vec::Vec;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use heapless::spsc::Producer;
use sube::rpc::chainhead::{ChainEvent, ChainHead};

use crate::event::{Status, UiEvent};

// Twox128("CollatorSelection") ++ Twox128("LastAuthoredBlock")
const KEY_PREFIX: [u8; 32] = [
    0x15, 0x46, 0x4c, 0xac, 0x33, 0x78, 0xd4, 0x6f, 0x11, 0x3c, 0xd5, 0xb7, 0xa4, 0xd7, 0x1c,
    0x84, 0xfb, 0x8e, 0xc9, 0x65, 0x6b, 0xa1, 0x6a, 0xc6, 0x22, 0x3a, 0x82, 0x47, 0x0e, 0x54,
    0x83, 0x7f,
];

/// Twox64Concat(account_id) suffixes for 6 active Kreivo collators.
const COLLATOR_SUFFIXES: [[u8; 40]; 6] = [
    [
        0x56, 0x58, 0xf6, 0xa0, 0x2a, 0x76, 0x00, 0xab, 0xc6, 0x70, 0xc3, 0x51, 0xe1, 0xd7,
        0x9a, 0xb5, 0x64, 0xae, 0xe1, 0xf5, 0x86, 0x97, 0xa7, 0x5f, 0x9f, 0xc8, 0xee, 0xd9,
        0xbf, 0xc4, 0xb0, 0x4c, 0x49, 0xb0, 0x6e, 0x2d, 0x0e, 0xe9, 0xce, 0x55,
    ],
    [
        0x58, 0xfd, 0xca, 0xde, 0x70, 0x5c, 0x50, 0x78, 0xc6, 0x6b, 0xed, 0x21, 0xf8, 0x87,
        0x6e, 0xf1, 0x20, 0xee, 0x46, 0x62, 0xb8, 0xc9, 0x04, 0xcf, 0x94, 0x75, 0xde, 0x4a,
        0xed, 0xbf, 0xed, 0xde, 0x35, 0x00, 0x1a, 0x48, 0xa6, 0x89, 0x86, 0x48,
    ],
    [
        0x76, 0xde, 0xc7, 0x34, 0xe8, 0xfa, 0x3e, 0x61, 0x6a, 0x5a, 0xed, 0xef, 0xf5, 0xc2,
        0x63, 0x7f, 0x97, 0x6b, 0xe2, 0xfa, 0x3f, 0x47, 0x65, 0x86, 0xd6, 0x86, 0x3d, 0x1e,
        0xf6, 0x17, 0xa8, 0x8e, 0x90, 0x44, 0x9c, 0x2f, 0xe6, 0x9d, 0x76, 0x43,
    ],
    [
        0x89, 0xf3, 0xff, 0xdc, 0x95, 0xf8, 0x3e, 0x9b, 0x16, 0xc2, 0xc8, 0x38, 0xe8, 0x40,
        0x12, 0xa7, 0x46, 0x5a, 0x49, 0x65, 0xd8, 0xf4, 0x68, 0x69, 0x87, 0x16, 0xdd, 0x88,
        0x1d, 0x94, 0xdc, 0xc2, 0x6b, 0x7e, 0xec, 0x0c, 0x58, 0x10, 0xb7, 0x2b,
    ],
    [
        0xb5, 0xab, 0x04, 0x6b, 0x56, 0x13, 0xd5, 0x29, 0x4a, 0x6c, 0xf9, 0x47, 0xd9, 0x98,
        0xec, 0x5c, 0x8a, 0x63, 0x08, 0x73, 0xc5, 0xc0, 0x84, 0x23, 0xb6, 0x84, 0xa0, 0x95,
        0x40, 0x53, 0xb8, 0x74, 0x87, 0x11, 0x51, 0x88, 0x4f, 0x6d, 0xf8, 0x6d,
    ],
    [
        0xc2, 0xc8, 0x32, 0xf5, 0xf6, 0xf4, 0x65, 0x81, 0xcc, 0xc1, 0x6a, 0x7a, 0xeb, 0x52,
        0x51, 0xb8, 0x51, 0xc0, 0x8f, 0xd8, 0x20, 0x68, 0x18, 0x79, 0x9f, 0x12, 0xb2, 0x2e,
        0xa5, 0x09, 0x85, 0xdd, 0x0c, 0x0e, 0x99, 0x02, 0xda, 0x50, 0xc9, 0x3b,
    ],
];

/// Build full storage keys (prefix + suffix) for all collators.
fn collator_keys() -> Vec<Vec<u8>> {
    COLLATOR_SUFFIXES
        .iter()
        .map(|suffix| {
            let mut key = vec![0u8; 72];
            key[..32].copy_from_slice(&KEY_PREFIX);
            key[32..].copy_from_slice(suffix);
            key
        })
        .collect()
}

/// Connect WiFi with retry loop.
pub async fn wifi_connect(
    controller: &mut esp_radio::wifi::WifiController<'static>,
    tx: &mut Producer<'static, UiEvent, 16>,
) {
    tx.enqueue(UiEvent::Status(Status::Dim("connecting wifi...")))
        .ok();
    loop {
        match controller.connect_async().await {
            Ok(()) => break,
            Err(e) => {
                log::warn!("WiFi: {:?}, retry in 5s", e);
                tx.enqueue(UiEvent::Status(Status::Error("wifi retry...")))
                    .ok();
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
    tx.enqueue(UiEvent::Status(Status::Dim("getting IP...")))
        .ok();
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
    tx.enqueue(UiEvent::Status(Status::Dim("connecting...")))
        .ok();

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
    tx.enqueue(UiEvent::Status(Status::Dim("websocket...")))
        .ok();
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
    tx.enqueue(UiEvent::Status(Status::Good(""))).ok();

    let mut block_count = 0u32;

    loop {
        match chain.next_chain_event().await {
            Ok(ChainEvent::NewBlock { hash, .. }) => {
                if let Ok(header) = chain.header(&hash).await {
                    tx.enqueue(UiEvent::Block(header.number as u32)).ok();
                }
                // Query collator storage every 5th block to reduce heap pressure
                block_count += 1;
                if block_count % 5 == 1 {
                    let keys = collator_keys();
                    match chain.get_storage_at_hash(&hash, keys.clone()).await {
                        Ok(items) => {
                            let mut blocks = [0u32; 6];
                            for (key, value) in &items {
                                if let Some(i) = keys.iter().position(|k| k == key) {
                                    if let Some(val) = value {
                                        if val.len() >= 4 {
                                            blocks[i] = u32::from_le_bytes([
                                                val[0], val[1], val[2], val[3],
                                            ]);
                                        }
                                    }
                                }
                            }
                            tx.enqueue(UiEvent::Collators(blocks)).ok();
                        }
                        Err(e) => log::warn!("storage query: {e}"),
                    }
                }
            }
            Ok(ChainEvent::Finalized { .. }) => {}
            Ok(_) => {}
            Err(e) => {
                log::error!("Chain: {e}");
                return Err("chain disconnected");
            }
        }
    }
}
