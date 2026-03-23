//! Benchmarks for metadata parsing and SCALE decoding.
//!
//! Run with: cargo bench --features json,text

use std::hint::black_box;
use std::time::Instant;

use sube::{Metadata, StorageEntry};

const META_BYTES: &[u8] = include_bytes!("../../../sdk/js/.papi/metadata/kreivo.scale");
const ITERATIONS: u32 = 100;

fn bench<F: FnMut()>(name: &str, mut f: F) {
    // Warm up
    for _ in 0..3 {
        f();
    }
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        f();
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / ITERATIONS;
    println!("{name}: {per_iter:?} per iteration ({ITERATIONS} iterations)");
}

fn main() {
    println!("sube benchmarks (metadata: {} bytes)\n", META_BYTES.len());

    // Parse metadata from SCALE bytes
    bench("metadata_parse", || {
        let _ = black_box(Metadata::from_bytes(META_BYTES).unwrap());
    });

    let meta = Metadata::from_bytes(META_BYTES).unwrap();

    // Look up a pallet by name
    bench("pallet_lookup", || {
        let _ = black_box(meta.pallet_by_name("System"));
    });

    // Decode a constant (System::Version)
    let system = meta.pallet_by_name("System").unwrap();
    let version = system
        .constants
        .iter()
        .find(|c| c.name == "Version")
        .unwrap();
    let entry = StorageEntry::new(version.value.clone(), version.ty);

    bench("constant_decode_json", || {
        let _ = black_box(entry.to_json(&meta.registry).unwrap());
    });

    bench("constant_decode_text", || {
        let _ = black_box(entry.to_text(&meta.registry).unwrap());
    });

    println!("\npallets: {}", meta.pallets.len());
}
