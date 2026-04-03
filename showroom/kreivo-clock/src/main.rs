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

// Twox128("CollatorSelection") ++ Twox128("LastAuthoredBlock")
const KEY_PREFIX: [u8; 32] = [
    0x15, 0x46, 0x4c, 0xac, 0x33, 0x78, 0xd4, 0x6f, 0x11, 0x3c, 0xd5, 0xb7, 0xa4, 0xd7, 0x1c,
    0x84, 0xfb, 0x8e, 0xc9, 0x65, 0x6b, 0xa1, 0x6a, 0xc6, 0x22, 0x3a, 0x82, 0x47, 0x0e, 0x54,
    0x83, 0x7f,
];

const COLLATOR_SUFFIXES: [[u8; 40]; 6] = [
    [0x56, 0x58, 0xf6, 0xa0, 0x2a, 0x76, 0x00, 0xab, 0xc6, 0x70, 0xc3, 0x51, 0xe1, 0xd7, 0x9a,
     0xb5, 0x64, 0xae, 0xe1, 0xf5, 0x86, 0x97, 0xa7, 0x5f, 0x9f, 0xc8, 0xee, 0xd9, 0xbf, 0xc4,
     0xb0, 0x4c, 0x49, 0xb0, 0x6e, 0x2d, 0x0e, 0xe9, 0xce, 0x55],
    [0x58, 0xfd, 0xca, 0xde, 0x70, 0x5c, 0x50, 0x78, 0xc6, 0x6b, 0xed, 0x21, 0xf8, 0x87, 0x6e,
     0xf1, 0x20, 0xee, 0x46, 0x62, 0xb8, 0xc9, 0x04, 0xcf, 0x94, 0x75, 0xde, 0x4a, 0xed, 0xbf,
     0xed, 0xde, 0x35, 0x00, 0x1a, 0x48, 0xa6, 0x89, 0x86, 0x48],
    [0x76, 0xde, 0xc7, 0x34, 0xe8, 0xfa, 0x3e, 0x61, 0x6a, 0x5a, 0xed, 0xef, 0xf5, 0xc2, 0x63,
     0x7f, 0x97, 0x6b, 0xe2, 0xfa, 0x3f, 0x47, 0x65, 0x86, 0xd6, 0x86, 0x3d, 0x1e, 0xf6, 0x17,
     0xa8, 0x8e, 0x90, 0x44, 0x9c, 0x2f, 0xe6, 0x9d, 0x76, 0x43],
    [0x89, 0xf3, 0xff, 0xdc, 0x95, 0xf8, 0x3e, 0x9b, 0x16, 0xc2, 0xc8, 0x38, 0xe8, 0x40, 0x12,
     0xa7, 0x46, 0x5a, 0x49, 0x65, 0xd8, 0xf4, 0x68, 0x69, 0x87, 0x16, 0xdd, 0x88, 0x1d, 0x94,
     0xdc, 0xc2, 0x6b, 0x7e, 0xec, 0x0c, 0x58, 0x10, 0xb7, 0x2b],
    [0xb5, 0xab, 0x04, 0x6b, 0x56, 0x13, 0xd5, 0x29, 0x4a, 0x6c, 0xf9, 0x47, 0xd9, 0x98, 0xec,
     0x5c, 0x8a, 0x63, 0x08, 0x73, 0xc5, 0xc0, 0x84, 0x23, 0xb6, 0x84, 0xa0, 0x95, 0x40, 0x53,
     0xb8, 0x74, 0x87, 0x11, 0x51, 0x88, 0x4f, 0x6d, 0xf8, 0x6d],
    [0xc2, 0xc8, 0x32, 0xf5, 0xf6, 0xf4, 0x65, 0x81, 0xcc, 0xc1, 0x6a, 0x7a, 0xeb, 0x52, 0x51,
     0xb8, 0x51, 0xc0, 0x8f, 0xd8, 0x20, 0x68, 0x18, 0x79, 0x9f, 0x12, 0xb2, 0x2e, 0xa5, 0x09,
     0x85, 0xdd, 0x0c, 0x0e, 0x99, 0x02, 0xda, 0x50, 0xc9, 0x3b],
];

fn collator_keys() -> alloc::vec::Vec<alloc::vec::Vec<u8>> {
    use alloc::vec;
    COLLATOR_SUFFIXES.iter().map(|suffix| {
        let mut key = vec![0u8; 72];
        key[..32].copy_from_slice(&KEY_PREFIX);
        key[32..].copy_from_slice(suffix);
        key
    }).collect()
}

static NET: sube::EdgeNet = sube::EdgeNet::new();


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
    // Connect without metadata — registry (60KB) + TLS (47KB) don't fit
    // in 115KB heap simultaneously. Use raw key queries instead.
    tx.enqueue(UiEvent::Status(Status::Dim("connecting...")))
        .ok();
    let rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG")?;
    let mut chain = sube::connect_edge("wss://kreivo.io", &NET, rng, &[])
        .await
        .map_err(|e| {
            log::error!("connect: {e}");
            "connect failed"
        })?;

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
                    let keys = collator_keys();
                    match chain
                        .backend()
                        .get_storage_at_hash(&hash, keys.clone())
                        .await
                    {
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
            Ok(_) => {}
            Err(e) => {
                log::error!("chain event: {e}");
                return Err("chain disconnected");
            }
        }
    }
}
