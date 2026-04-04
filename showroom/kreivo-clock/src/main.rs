//! Kreivo Clock — live blockchain data on your wrist
//!
//! Showcases sube on an ESP32-S3 (T-Watch S3).
//! Metadata is pushed from a phone browser via HTTP — the device
//! serves a config page that fetches metadata from the chain and
//! uploads it to the watch.
//!
//! Flash: espflash flash -p /dev/ttyACM0 -M target/xtensa-esp32s3-none-elf/release/kreivo-clock

#![no_std]
#![no_main]

extern crate alloc;
extern crate tinyrlibc;

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use sube::ChainEvent;

use kreivo_clock::device::event::{Status, UiEvent};
use kreivo_clock::http;

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

    spawner.spawn(http::http_task(rt.stack)).ok();
    if let Some(cfg) = rt.stack.config_v4() {
        let ip = cfg.address.address();
        log::info!("config: http://{ip}/");
        let mut url = heapless::String::<32>::new();
        core::fmt::Write::write_fmt(&mut url, format_args!("http://{ip}/")).ok();
        rt.events.enqueue(UiEvent::ConfigUrl(url)).ok();
    }

    let meta = if let Some(raw) = kreivo_clock::flash::load() {
        log::info!("decoding flash metadata ({} bytes)...", raw.len());
        match sube::metadata::from_bytes_filtered(&raw, &["CollatorSelection"]) {
            Ok(meta) => {
                log::info!("flash metadata: {} pallets", meta.pallets.len());
                http::PALLET_COUNT
                    .store(meta.pallets.len() as u8, core::sync::atomic::Ordering::Relaxed);
                http::META_SAVED.store(true, core::sync::atomic::Ordering::Relaxed);
                Some(meta)
            }
            Err(e) => {
                log::warn!("flash metadata invalid: {:?}", e);
                None
            }
        }
    } else {
        None
    };

    let meta = if let Some(meta) = meta {
        meta
    } else {
        // Wait for metadata push via HTTP
        log::info!("waiting for metadata via http...");
        loop {
            if let Some(raw) = http::METADATA_SLOT.take() {
                log::info!("decoding {} bytes of metadata...", raw.len());
                match sube::metadata::from_bytes_filtered(&raw, &["CollatorSelection"]) {
                    Ok(meta) => {
                        // Save to flash for next boot
                        if let Err(e) = kreivo_clock::flash::save(&raw) {
                            log::warn!("flash save failed: {e}");
                        } else {
                            http::META_SAVED.store(true, core::sync::atomic::Ordering::Relaxed);
                        }

                        log::info!("metadata decoded ({} pallets)", meta.pallets.len());
                        http::PALLET_COUNT
                            .store(meta.pallets.len() as u8, core::sync::atomic::Ordering::Relaxed);
                        break meta;
                    }
                    Err(e) => {

                        log::error!("metadata decode failed: {:?}", e);
                        rt.events
                            .enqueue(UiEvent::Status(Status::Error("bad metadata")))
                            .ok();
                    }
                }
            }
            Timer::after(Duration::from_millis(200)).await;
        }
    };
    let meta = alloc::sync::Arc::new(meta);

    let mut retries = 0u8;
    loop {
        rt.events
            .enqueue(UiEvent::Status(Status::Dim("connecting...")))
            .ok();
        match watch_chain(&mut rt, &meta).await {
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
    meta: &alloc::sync::Arc<sube::Metadata>,
) -> Result<(), &'static str> {
    let tx = &mut rt.events;

    let rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG")?;
    let mut chain = sube::connect_edge("wss://kreivo.io", &NET, rng, &[])
        .await
        .map_err(|e| {
            log::error!("connect: {e}");
            "connect failed"
        })?;
    log::info!("heap after connect: {} free", esp_alloc::HEAP.free());

    *chain.metadata_mut() = alloc::sync::Arc::clone(meta);

    tx.enqueue(UiEvent::Live(true)).ok();
    tx.enqueue(UiEvent::Status(Status::Good(""))).ok();

    let mut block_count = 0u32;

    loop {
        // Check for hot-swapped metadata from HTTP
        if let Some(raw) = http::METADATA_SLOT.take() {
            if let Ok(meta) = sube::metadata::from_bytes_filtered(&raw, &["CollatorSelection"]) {
                log::info!("hot-swapped metadata ({} pallets)", meta.pallets.len());
                *chain.metadata_mut() = alloc::sync::Arc::new(meta);
            }
            drop(raw);
        }

        match chain.next_event().await {
            Ok(ChainEvent::NewBlock { hash, .. }) => {
                block_count += 1;
                let screen = kreivo_clock::SCREEN_ON.load(core::sync::atomic::Ordering::Relaxed);

                // Skip all RPC queries when screen is off — saves bandwidth + CPU.
                // Chain events still arrive (keeping the subscription alive).
                if !screen {
                    continue;
                }

                if let Ok(header) = chain.header(&hash).await {
                    let num = header.number as u32;
                    tx.enqueue(UiEvent::Block(num)).ok();
                    http::BLOCK_NUMBER.store(num, core::sync::atomic::Ordering::Relaxed);
                }

                let has_meta = !chain.metadata().pallets.is_empty();
                if has_meta && block_count % 5 == 1 {
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
