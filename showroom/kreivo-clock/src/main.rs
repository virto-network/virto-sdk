//! Kreivo Clock — live blockchain data on your wrist
//!
//! Showcases sube on an ESP32-S3 (T-Watch S3).
//! `sube::connect_edge` handles the full connection and metadata stack
//! (DNS → TCP → TLS → WebSocket → ChainHead → streaming filtered metadata).
//! Queries use human-readable pallet/storage paths.
//!
//! WiFi is managed by the firmware in the background — the app only
//! cares about the chain connection.
//!
//! Flash: espflash flash -p /dev/ttyACM0 -M target/xtensa-esp32s3-none-elf/release/kreivo-clock

#![no_std]
#![no_main]

extern crate alloc;
extern crate tinyrlibc;

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
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

static NET: sube::EdgeNet = sube::EdgeNet::new();


#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let mut rt = kreivo_clock::start(spawner).await;
    NET.init(rt.stack);

    let mut retries = 0u8;
    loop {
        match watch_chain(&mut rt).await {
            Ok(()) => retries = 0,
            Err(e) => {
                log::error!("chain: {e}");
                retries += 1;
                if retries > 3 {
                    log::error!("too many failures, rebooting");
                    esp_hal::system::software_reset();
                }
                rt.events.enqueue(UiEvent::Live(false)).ok();
                rt.events
                    .enqueue(UiEvent::Status(Status::Error("reconnecting...")))
                    .ok();
                Timer::after(Duration::from_secs(3)).await;
            }
        }
    }
}

// ── sube: chain watcher ────────────────────────────────────────────────

async fn watch_chain(
    rt: &mut kreivo_clock::Runtime,
) -> Result<(), &'static str> {
    let tx = &mut rt.events;
    // Registry was pre-allocated at boot (before WiFi) from clean heap.
    // Single connection: scan → decode → use.
    tx.enqueue(UiEvent::Status(Status::Dim("loading metadata...")))
        .ok();
    let rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG")?;
    let mut tmp = sube::connect_edge("wss://kreivo.io", &NET, rng, &[])
        .await
        .map_err(|e| {
            log::error!("connect: {e}");
            "connect failed"
        })?;
    log::info!("heap after connect: {} free", esp_alloc::HEAP.free());

    let (needed_ids, type_count) = tmp
        .backend()
        .metadata_scan_pallets(&["CollatorSelection"])
        .await
        .map_err(|e| {
            log::error!("scan: {e}");
            "scan failed"
        })?;

    let meta = tmp
        .backend()
        .metadata_decode_filtered(
            &["CollatorSelection"],
            &needed_ids,
            type_count,
            &mut rt.registry,
        )
        .await
        .map_err(|e| {
            log::error!("decode: {e}");
            "decode failed"
        })?;
    drop(needed_ids);
    *tmp.metadata_mut() = alloc::sync::Arc::new(meta);
    let mut chain = tmp;
    log::info!(
        "metadata loaded ({} pallets), heap: {} free",
        chain.metadata().pallets.len(),
        esp_alloc::HEAP.free()
    );

    tx.enqueue(UiEvent::Live(true)).ok();
    tx.enqueue(UiEvent::Status(Status::Good(""))).ok();

    let mut block_count = 0u32;

    loop {
        match chain.next_event().await {
            Ok(ChainEvent::NewBlock { hash, .. }) => {
                if let Ok(header) = chain.header(&hash).await {
                    tx.enqueue(UiEvent::Block(header.number as u32)).ok();
                }
                block_count += 1;
                if block_count % 5 == 1 {
                    let mut blocks = [0u32; 6];
                    for (i, addr) in COLLATORS.iter().enumerate() {
                        let path =
                            alloc::format!("collator-selection/last-authored-block/{addr}");
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
                log::error!("chain event: {e}");
                return Err("chain disconnected");
            }
        }
    }
}
