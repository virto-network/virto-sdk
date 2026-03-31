//! Kreivo Clock — live blockchain data on your wrist
//!
//! Showcases sube on an ESP32-S3 (T-Watch S3).
//! `sube::connect_edge` handles the full connection and metadata stack in one
//! call (DNS → TCP → TLS → WebSocket → ChainHead → streaming filtered metadata).
//! Queries use human-readable pallet/storage paths.
//!
//! Device setup lives in the library crate.
//!
//! Flash: espflash flash -p /dev/ttyACM0 -M target/xtensa-esp32s3-none-elf/release/kreivo-clock

#![no_std]
#![no_main]

extern crate alloc;
extern crate tinyrlibc;

use embassy_executor::Spawner;
use esp_backtrace as _;
use heapless::spsc::Producer;
use sube::ChainEvent;

use kreivo_clock::device::event::{Status, UiEvent};

esp_bootloader_esp_idf::esp_app_desc!();

/// Hex-encoded public keys of the 6 active Kreivo collators.
const COLLATORS: [&str; 6] = [
    "0x64aee1f58697a75f9fc8eed9bfc4b04c49b06e2d0ee9ce55c6e1deb5a70a1546",
    "0x20ee4662b8c904cf9475de4aedbfedde35001a48a6898648f8876ef1c66bed21",
    "0x976be2fa3f476586d6863d1ef617a88e90449c2fe69d764397e2c063f5c7de76",
    "0x465a4965d8f46869871688d81d94dcc26b7eec0c5810b72be84012a7c8c238e8",
    "0x8a630873c5c08423b684a09540538874871151884f6df86d4a6cf947d998ec5c",
    "0x51c08fd82068187f9f12b22ea50985dd0c0e9902da50c93beb5251b8cc6a7aeb",
];

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let mut rt = kreivo_clock::start(spawner).await;
    rt.connect().await;

    // watch_chain connects once and runs until disconnect.
    // On failure, reconnect WiFi only if needed, then retry.
    // edge_connect leaks memory for TLS buffers, so we can't
    // retry indefinitely — reboot after a few failures.
    let mut retries = 0u8;
    loop {
        match watch_chain(rt.stack, &mut rt.events).await {
            Ok(()) => retries = 0,
            Err(e) => {
                log::error!("{e}");
                retries += 1;
                if retries > 3 {
                    log::error!("too many failures, rebooting");
                    esp_hal::system::software_reset();
                }
                rt.report_disconnected().await;
                if !rt.stack.is_link_up() {
                    rt.disconnect().await;
                    rt.connect().await;
                }
            }
        }
    }
}

// ── sube: chain watcher ────────────────────────────────────────────────

async fn watch_chain(
    stack: embassy_net::Stack<'static>,
    tx: &mut Producer<'static, UiEvent, 16>,
) -> Result<(), &'static str> {
    tx.enqueue(UiEvent::Status(Status::Dim("connecting...")))
        .ok();

    log::info!("watch_chain: starting connection");
    let rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG")?;
    let mut chain = sube::connect_edge("wss://kreivo.io", stack, rng, &[])
        .await
        .map_err(|e| {
            log::error!("watch_chain: connect failed: {e}");
            "sube connect failed"
        })?;
    log::info!("watch_chain: connected, loading metadata");

    tx.enqueue(UiEvent::Status(Status::Dim("loading metadata...")))
        .ok();
    chain
        .backend()
        .metadata_filtered_streaming(&["CollatorSelection"])
        .await
        .map(|m| *chain.metadata_mut() = alloc::sync::Arc::new(m))
        .map_err(|e| {
            log::error!("watch_chain: metadata failed: {e}");
            "metadata failed"
        })?;
    log::info!("watch_chain: metadata loaded, starting event loop");

    tx.enqueue(UiEvent::Live(true)).ok();
    tx.enqueue(UiEvent::Status(Status::Good(""))).ok();

    let mut block_count = 0u32;

    loop {
        match chain.next_event().await {
            Ok(ChainEvent::NewBlock { hash, .. }) => {
                if let Ok(header) = chain.header(&hash).await {
                    tx.enqueue(UiEvent::Block(header.number as u32)).ok();
                }
                // Query collator storage every 5th block to reduce heap pressure
                block_count += 1;
                if block_count % 5 == 1 {
                    let mut blocks = [0u32; 6];
                    for (i, addr) in COLLATORS.iter().enumerate() {
                        let path = alloc::format!("collator-selection/last-authored-block/{addr}");
                        if let Ok((entry, _)) = chain
                            .query_at_hash(&path, &hash)
                            .await
                            .and_then(|r| r.into_value())
                        {
                            blocks[i] = entry.as_u32().unwrap_or(0);
                        }
                    }
                    tx.enqueue(UiEvent::Collators(blocks)).ok();
                }
            }
            Ok(_) => {}
            Err(e) => {
                log::error!("Chain: {e}");
                return Err("chain disconnected");
            }
        }
    }
}
