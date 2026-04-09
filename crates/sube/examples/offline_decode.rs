//! Offline metadata + storage decoding — the embedded use case.
//!
//! This example doesn't talk to any node. It shows the parts of sube that run
//! on `no_std` hardware (parse metadata, look up a pallet, encode a storage
//! key, decode a SCALE blob to text) against bundled fixtures.
//!
//! The same code works identically on an ESP32 / Cortex-M target — only the
//! transport that delivers the raw bytes differs.
//!
//! Run with: cargo run --example offline_decode

use sube::{Metadata, StorageEntry};

fn main() -> sube::Result<()> {
    // Metadata that would normally be fetched once and cached in flash.
    let meta = Metadata::from_bytes(include_bytes!("../tests/fixtures/kreivo.scale"))?;
    println!("loaded metadata: {} pallets", meta.pallets.len());

    // Look up a constant from the System pallet — no network needed.
    let system = meta
        .pallet_by_name("System")
        .ok_or(sube::Error::PalletNotFound("System".into()))?;
    let version = system
        .constants
        .iter()
        .find(|c| c.name == "Version")
        .ok_or_else(|| sube::Error::ConstantNotFound("Version".into()))?;

    let entry = StorageEntry::new(version.value.clone(), version.ty);
    let text = entry.to_text(&meta.registry)?;
    println!("System::Version = {text}");

    // Filtered metadata: keep only System + Balances to shrink the in-memory
    // registry for tight embedded budgets.
    let filtered = Metadata::from_bytes_filtered(
        include_bytes!("../tests/fixtures/kreivo.scale"),
        &["Balances"],
    )?;
    println!(
        "filtered metadata: {} pallets (was {})",
        filtered.pallets.len(),
        meta.pallets.len()
    );

    Ok(())
}
