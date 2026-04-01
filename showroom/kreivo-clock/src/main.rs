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

static NET: sube::EdgeNet = sube::EdgeNet::new();

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
    NET.init(rt.stack);

    let mut retries = 0u8;
    loop {
        match watch_chain(&mut rt.events).await {
            Ok(()) => retries = 0,
            Err(e) => {
                log::error!("chain: {e}");
                retries += 1;
                if retries > 3 {
                    log::error!("too many failures, rebooting");
                    esp_hal::system::software_reset();
                }
                rt.events
                    .enqueue(UiEvent::Status(Status::Error("reconnecting...")))
                    .ok();
                rt.events.enqueue(UiEvent::Live(false)).ok();
                Timer::after(Duration::from_secs(3)).await;
            }
        }
    }
}

// ── sube: chain watcher ────────────────────────────────────────────────

async fn watch_chain(
    tx: &mut Producer<'static, UiEvent, 16>,
) -> Result<(), &'static str> {
    use alloc::boxed::Box;

    // Step 1: scan pallets (Box::pin to keep future size small)
    tx.enqueue(UiEvent::Status(Status::Dim("scanning pallets...")))
        .ok();
    let (needed_ids, type_count): (alloc::collections::BTreeSet<u32>, u32) = Box::pin(async {
        let rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG")?;
        let mut tmp = sube::connect_edge("wss://kreivo.io", &NET, rng, &[])
            .await
            .map_err(|e| { log::error!("connect (scan): {e}"); "connect failed" })?;
        tmp.backend()
            .metadata_scan_pallets(&["CollatorSelection"])
            .await
            .map_err(|e| { log::error!("scan: {e}"); "scan failed" })
    })
    .await?;

    // Step 2: decode types on fresh connection.
    // NOT Box::pin'd — saves ~25KB heap vs Box::pin'd version.
    // Stack is sufficient at 168KB heap (8KB more than 176KB which overflowed).
    tx.enqueue(UiEvent::Status(Status::Dim("loading types...")))
        .ok();
    // Pause UI rendering to free Slint's scene allocations (~20KB)
    kreivo_clock::PAUSE_UI.store(true, core::sync::atomic::Ordering::Relaxed);
    // Give core 1 a moment to finish its current render cycle
    embassy_time::Timer::after(embassy_time::Duration::from_millis(100)).await;
    log::info!("heap before step 2: {} free", esp_alloc::HEAP.free());
    let metadata = {
        let rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG")?;
        let mut tmp = sube::connect_edge("wss://kreivo.io", &NET, rng, &[])
            .await
            .map_err(|e| {
                log::error!("connect (decode): {e}");
                "connect failed"
            })?;
        log::info!("heap after connect: {} free", esp_alloc::HEAP.free());
        // Preallocate only strings + str_idx (the two that cause the worst
        // doubling: strings 16→32KB = 48KB transient, str_idx 1024→2048 = 18KB transient).
        // Budget: ~68KB free. Target: ~32KB prealloc, ~36KB remaining.
        // Preallocate tight: strings(16KB) + str_idx(11KB) + variants(10KB) = ~37KB
        // Leaves ~31KB for: fields growth(max 12KB) + BTreeMap(3KB) + types(3KB) + temps
        let mut registry = sube::Registry::with_capacity_detailed(0, 0, 0, 0, 16000);
        registry.reserve_str_idx(1800);
        registry.reserve_variants(850);
        registry.reserve_fields(1100);
        log::info!("heap after prealloc: {} free", esp_alloc::HEAP.free());
        let meta = tmp
            .backend()
            .metadata_decode_filtered(
                &["CollatorSelection"],
                &needed_ids,
                type_count,
                &mut registry,
            )
            .await
            .map_err(|e| {
                log::error!("decode: {e}");
                "decode failed"
            })?;
        alloc::sync::Arc::new(meta)
    };
    drop(needed_ids);
    // Resume UI rendering
    kreivo_clock::PAUSE_UI.store(false, core::sync::atomic::Ordering::Relaxed);
    log::info!("metadata loaded, reconnecting for events");

    // Step 3: final connection with loaded metadata
    tx.enqueue(UiEvent::Status(Status::Dim("connecting...")))
        .ok();
    let rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG")?;
    let mut chain = sube::connect_edge("wss://kreivo.io", &NET, rng, &[])
        .await
        .map_err(|e| {
            log::error!("connect (final): {e}");
            "connect failed"
        })?;
    *chain.metadata_mut() = metadata;

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
