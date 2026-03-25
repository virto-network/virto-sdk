//! Memory usage test for metadata parsing.
//!
//! Run with: cargo run --example memory_test --features json,text --release

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TrackingAlloc;
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let current = ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(current, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static A: TrackingAlloc = TrackingAlloc;

const META_BYTES: &[u8] = include_bytes!("../../../sdk/js/.papi/metadata/kreivo.scale");

fn count_types(meta: &sube::Metadata) -> u32 {
    let mut n = 0u32;
    while meta.registry.resolve(n).is_some() {
        n += 1;
    }
    n
}

fn main() {
    println!("Kreivo metadata: {} KB SCALE\n", META_BYTES.len() / 1024);

    // Full parse
    ALLOCATED.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);

    let meta = sube::Metadata::from_bytes(META_BYTES).unwrap();
    println!("--- Full parse ---");
    println!("  peak:     {} KB", PEAK.load(Ordering::Relaxed) / 1024);
    println!("  retained: {} KB", ALLOCATED.load(Ordering::Relaxed) / 1024);
    println!("  pallets: {}, types: {}", meta.pallets.len(), count_types(&meta));
    drop(meta);

    // Filtered parse
    ALLOCATED.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);

    let meta = sube::Metadata::from_bytes_filtered(META_BYTES, &["Balances"]).unwrap();
    println!("\n--- Filtered parse (System + Balances) ---");
    println!("  peak:     {} KB", PEAK.load(Ordering::Relaxed) / 1024);
    println!("  retained: {} KB", ALLOCATED.load(Ordering::Relaxed) / 1024);
    println!("  pallets: {}, types: {}", meta.pallets.len(), count_types(&meta));
    drop(meta);

    // Minimal: just one storage query pallet
    ALLOCATED.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);

    let meta = sube::Metadata::from_bytes_filtered(META_BYTES, &["Timestamp"]).unwrap();
    println!("\n--- Filtered parse (System + Timestamp) ---");
    println!("  peak:     {} KB", PEAK.load(Ordering::Relaxed) / 1024);
    println!("  retained: {} KB", ALLOCATED.load(Ordering::Relaxed) / 1024);
    println!("  pallets: {}, types: {}", meta.pallets.len(), count_types(&meta));

    // Lean decoder (no frame_metadata dependency)
    ALLOCATED.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);

    let raw = sube::scales::frame::metadata::decode_metadata(META_BYTES).unwrap();
    println!("\n--- Lean decoder (all types, no frame_metadata) ---");
    println!("  peak:     {} KB", PEAK.load(Ordering::Relaxed) / 1024);
    println!("  retained: {} KB", ALLOCATED.load(Ordering::Relaxed) / 1024);
    println!("  pallets: {}, types: {}", raw.pallets.len(), raw.types.len());
    drop(raw);

    // Lean decoder with two-pass filtering
    ALLOCATED.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);

    let raw = sube::scales::frame::metadata::decode_metadata_filtered(META_BYTES, &["Balances"]).unwrap();
    println!("\n--- Lean two-pass filtered (System + Balances) ---");
    println!("  peak:     {} KB", PEAK.load(Ordering::Relaxed) / 1024);
    println!("  retained: {} KB", ALLOCATED.load(Ordering::Relaxed) / 1024);
    println!("  pallets: {}, types: {}", raw.pallets.len(), raw.types.len());
    drop(raw);

    // Minimal
    ALLOCATED.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);

    let raw = sube::scales::frame::metadata::decode_metadata_filtered(META_BYTES, &["Timestamp"]).unwrap();
    println!("\n--- Lean two-pass filtered (System + Timestamp) ---");
    println!("  peak:     {} KB", PEAK.load(Ordering::Relaxed) / 1024);
    println!("  retained: {} KB", ALLOCATED.load(Ordering::Relaxed) / 1024);
    println!("  pallets: {}, types: {}", raw.pallets.len(), raw.types.len());
    drop(raw);

    println!("\nESP32 SRAM budget: 320 KB");
    println!("ESP32 PSRAM budget: 4096 KB");
}
