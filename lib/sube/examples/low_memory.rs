//! End-to-end low-memory demo — connect, fetch filtered metadata,
//! query storage, watch blocks, all with memory tracking.
//!
//! Run: cargo run --example low_memory --features wss,text --release

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

fn mem() -> (usize, usize) {
    (
        ALLOCATED.load(Ordering::Relaxed),
        PEAK.load(Ordering::Relaxed),
    )
}

fn reset_peak() {
    PEAK.store(ALLOCATED.load(Ordering::Relaxed), Ordering::Relaxed);
}

fn kb(n: usize) -> usize {
    n / 1024
}

fn main() {
    smol::block_on(async {
        println!("=== sube low-memory e2e demo ===\n");

        // Phase 1: Connect with filtered metadata (WS + chainHead + metadata in one shot)
        reset_peak();
        let (before, _) = mem();
        println!("[1] Connecting with filtered metadata (System + Balances)...");
        let mut chain = sube::Sube::connect_filtered("wss://kreivo.io", &["Balances"])
            .await
            .expect("connect");
        let (after, peak) = mem();
        println!("    pallets: {}", chain.metadata().pallets.len());
        println!(
            "    connected: +{} KB retained, {} KB peak\n",
            kb(after - before),
            kb(peak)
        );

        // Phase 3: Query storage
        reset_peak();
        let (before, _) = mem();
        println!("[3] Querying system/number...");
        let response = chain.query("system/number").await.expect("query");
        if let Some(text) = response.to_text().expect("decode") {
            println!("    block number: {text}");
        }
        let (after, peak) = mem();
        println!(
            "    query: +{} KB retained, {} KB peak\n",
            kb(after.saturating_sub(before)),
            kb(peak)
        );

        // Phase 4: Query account
        reset_peak();
        println!("[4] Querying balances/account...");
        let addr = "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b";
        let response = chain
            .query(&format!("system/account/{addr}"))
            .await
            .expect("query account");
        if let Some(text) = response.to_text().expect("decode") {
            println!("    account: {text}");
        }
        let (after, peak) = mem();
        println!(
            "    query: +{} KB retained, {} KB peak\n",
            kb(after.saturating_sub(before)),
            kb(peak)
        );

        // Phase 5: Watch blocks
        reset_peak();
        println!("[5] Watching 5 blocks...");
        for i in 0..5 {
            match chain.next_event().await {
                Ok(sube::ChainEvent::NewBlock { hash, .. }) => {
                    let header = chain.header(&hash).await.ok();
                    let num = header.map(|h| h.number).unwrap_or(0);
                    println!("    block #{num}");
                }
                Ok(sube::ChainEvent::Finalized { hashes, .. }) => {
                    println!("    finalized {} blocks", hashes.len());
                }
                Ok(_) => {}
                Err(e) => {
                    println!("    error: {e}");
                    break;
                }
            }
            if i == 4 {
                break;
            }
        }
        let (_, peak) = mem();
        println!("    watch peak: {} KB\n", kb(peak));

        // Summary
        let (total, total_peak) = mem();
        println!("=== Memory Summary ===");
        println!("  total retained: {} KB", kb(total));
        println!("  total peak:     {} KB", kb(total_peak));
        println!("  ESP32 SRAM:     320 KB");
        println!("  headroom:       {} KB", 320i64 - kb(total) as i64);
    });
}
