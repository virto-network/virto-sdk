//! Metadata loading: try flash first, fall back to waiting for HTTP push.
//!
//! Once metadata arrives via HTTP, persist it to flash so subsequent
//! boots skip the wait entirely.

use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use embassy_time::{Duration, Timer};

use crate::device::event::{Status, UiEvent};
use crate::http;
use crate::Runtime;

const PALLETS: &[&str] = &["CollatorSelection"];

/// Load metadata from flash, or wait for an HTTP push from the config page.
pub async fn load_or_fetch(rt: &mut Runtime) -> sube::Metadata {
    if let Some(meta) = try_load_from_flash() {
        return meta;
    }

    log::info!("waiting for metadata via http...");
    loop {
        if let Some(raw) = http::METADATA_SLOT.take() {
            match decode(&raw) {
                Ok(meta) => {
                    if let Err(e) = crate::flash::save(&raw) {
                        log::warn!("flash save failed: {e}");
                    } else {
                        http::META_SAVED.store(true, Ordering::Relaxed);
                    }
                    return meta;
                }
                Err(e) => {
                    log::error!("metadata decode failed: {e:?}");
                    rt.events
                        .enqueue(UiEvent::Status(Status::Error("bad metadata")))
                        .ok();
                }
            }
        }
        Timer::after(Duration::from_millis(200)).await;
    }
}

/// Check if metadata was hot-swapped via HTTP since the last call.
pub fn take_pushed() -> Option<sube::Metadata> {
    let raw = http::METADATA_SLOT.take()?;
    match decode(&raw) {
        Ok(meta) => {
            // Persist the new metadata so it survives reboot
            if crate::flash::save(&raw).is_ok() {
                http::META_SAVED.store(true, Ordering::Relaxed);
            }
            Some(meta)
        }
        Err(e) => {
            log::warn!("hot-swap metadata invalid: {e:?}");
            None
        }
    }
}

fn try_load_from_flash() -> Option<sube::Metadata> {
    let raw = crate::flash::load()?;
    log::info!("decoding flash metadata ({} bytes)...", raw.len());
    match decode(&raw) {
        Ok(meta) => {
            log::info!("flash metadata: {} pallets", meta.pallets.len());
            http::META_SAVED.store(true, Ordering::Relaxed);
            Some(meta)
        }
        Err(e) => {
            log::warn!("flash metadata invalid: {e:?}");
            None
        }
    }
}

fn decode(raw: &[u8]) -> Result<sube::Metadata, sube::Error> {
    let meta = sube::metadata::from_bytes_filtered(raw, PALLETS)?;
    http::PALLET_COUNT.store(meta.pallets.len() as u8, Ordering::Relaxed);
    Ok(meta)
}

/// Receive bytes for type compat — keeps `Vec<u8>` accessible.
#[allow(dead_code)]
pub(crate) fn raw_bytes() -> Option<Vec<u8>> {
    http::METADATA_SLOT.take()
}
