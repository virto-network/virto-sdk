//! Kreivo Clock — sube on a watch.
//!
//! Connects to a Substrate chain over WebSocket+TLS, watches blocks,
//! and queries collator activity. Everything device-specific (display,
//! battery, WiFi, HTTP config server, metadata persistence) lives in
//! the library — `main.rs` is the sube showcase.
//!
//! Flash: espflash flash -p /dev/ttyACM0 -M target/xtensa-esp32s3-none-elf/release/kreivo-clock

#![no_std]
#![no_main]

extern crate alloc;
extern crate tinyrlibc;

use alloc::rc::Rc;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use sube::ChainEvent;

use kreivo_clock::device::event::Status;
use kreivo_clock::{metadata, Runtime};

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

    // Load metadata: from flash if present, otherwise wait for an HTTP push.
    let meta = Rc::new(metadata::load_or_fetch(&mut rt).await);

    let mut retries = 0u8;
    loop {
        rt.status(Status::Dim("connecting..."));
        match watch_chain(&mut rt, &meta).await {
            Ok(()) => retries = 0,
            Err(e) => {
                log::error!("chain: {e}");
                retries += 1;
                if retries > 3 {
                    log::error!("too many failures, rebooting");
                    esp_hal::system::software_reset();
                }
                rt.set_live(false);
                rt.status(Status::Error("reconnecting..."));
                Timer::after(Duration::from_secs(3)).await;
            }
        }
    }
}

// ── sube: chain watcher ────────────────────────────────────────────────

async fn watch_chain(rt: &mut Runtime, meta: &Rc<sube::Metadata>) -> Result<(), &'static str> {
    let rng = esp_hal::rng::Trng::try_new().map_err(|_| "TRNG")?;
    let mut chain = sube::connect_edge("wss://kreivo.io", &NET, rng, &[])
        .await
        .map_err(|e| {
            log::error!("connect: {e}");
            "connect failed"
        })?;
    *chain.metadata_mut() = Rc::clone(meta);

    rt.set_live(true);
    rt.status(Status::Good(""));

    let mut block_count = 0u32;
    loop {
        // Hot-swap metadata if a new push arrived
        if let Some(new_meta) = metadata::take_pushed() {
            log::info!("hot-swapped metadata ({} pallets)", new_meta.pallets.len());
            *chain.metadata_mut() = Rc::new(new_meta);
        }

        match chain.next_event().await {
            Ok(ChainEvent::NewBlock { hash, .. }) => {
                block_count += 1;
                // Skip queries when screen is off — saves bandwidth + CPU.
                if !rt.screen_on() {
                    continue;
                }

                if let Ok(header) = chain.header(&hash).await {
                    rt.set_block(header.number as u32);
                }

                let has_meta = !chain.metadata().pallets.is_empty();
                if has_meta && block_count % 5 == 1 {
                    rt.set_collators(query_collators(&mut chain, &hash).await);
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

/// Query each collator's last-authored-block via human-readable storage path.
async fn query_collators(
    chain: &mut sube::EdgeSube,
    hash: &str,
) -> [u32; 6] {
    let mut blocks = [0u32; 6];
    for (i, addr) in COLLATORS.iter().enumerate() {
        let path = alloc::format!("collator-selection/last-authored-block/{addr}");
        if let Ok((entry, _)) = chain
            .query_at_hash(&path, hash)
            .await
            .and_then(|r| r.into_value())
        {
            blocks[i] = entry.as_u32().unwrap_or(0);
        }
    }
    blocks
}
