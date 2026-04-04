//! Persist metadata to flash so the watch works after reboot.
//!
//! Uses a fixed region at the end of the 16MB flash.
//! Format: `[MAGIC:4][LENGTH:4][metadata_bytes:LENGTH]`

use alloc::vec;
use alloc::vec::Vec;
use embedded_storage::{ReadStorage, Storage};

/// Magic bytes to identify valid stored metadata.
const MAGIC: [u8; 4] = *b"KMET";

/// Flash offset: 15.5MB into 16MB flash (well past the app partition).
const FLASH_OFFSET: u32 = 15 * 1024 * 1024 + 512 * 1024;

/// Maximum metadata size we'll store (448KB).
const MAX_SIZE: usize = 448 * 1024;

/// Get a FlashStorage instance. Uses unsafe peripheral steal since
/// the FLASH peripheral is just a marker type on ESP32-S3.
fn open_flash() -> esp_storage::FlashStorage<'static> {
    unsafe {
        let flash = esp_hal::peripherals::FLASH::steal();
        esp_storage::FlashStorage::new(flash).multicore_ignore()
    }
}

/// Save raw metadata bytes to flash.
pub fn save(data: &[u8]) -> Result<(), &'static str> {
    if data.len() > MAX_SIZE {
        return Err("metadata too large for flash");
    }

    let mut flash = open_flash();
    let len = data.len() as u32;
    let mut header = [0u8; 8];
    header[..4].copy_from_slice(&MAGIC);
    header[4..8].copy_from_slice(&len.to_le_bytes());

    flash.write(FLASH_OFFSET, &header).map_err(|_| "flash write header")?;
    flash.write(FLASH_OFFSET + 8, data).map_err(|_| "flash write data")?;

    log::info!("flash: saved {} bytes of metadata", data.len());
    Ok(())
}

/// Load metadata bytes from flash, if previously stored.
pub fn load() -> Option<Vec<u8>> {
    let mut flash = open_flash();

    let mut header = [0u8; 8];
    flash.read(FLASH_OFFSET, &mut header).ok()?;

    if header[..4] != MAGIC {
        log::info!("flash: no stored metadata");
        return None;
    }

    let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    if len == 0 || len > MAX_SIZE {
        log::warn!("flash: invalid metadata length: {}", len);
        return None;
    }

    let mut data = vec![0u8; len];
    if flash.read(FLASH_OFFSET + 8, &mut data).is_err() {
        log::warn!("flash: read failed");
        return None;
    }

    log::info!("flash: loaded {} bytes of metadata", len);
    Some(data)
}
