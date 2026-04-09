//! QEMU smoke test — proves sube works on bare-metal Cortex-M4.
//!
//! Run: just test-qemu

#![no_std]
#![no_main]

extern crate alloc;
extern crate panic_semihosting;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use embedded_alloc::LlffHeap;

#[global_allocator]
static HEAP: LlffHeap = LlffHeap::empty();

#[entry]
fn main() -> ! {
    // Init heap
    {
        const HEAP_SIZE: usize = 32 * 1024;
        static mut HEAP_MEM: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
        unsafe { HEAP.init((&raw mut HEAP_MEM) as usize, HEAP_SIZE) }
    }

    let _ = hprintln!("=== sube QEMU smoke test ===");

    // Core types work on bare metal
    let _ = hprintln!("test: StorageEntry...");
    let entry = sube::StorageEntry::new(alloc::vec![1, 2, 3, 4], 0);
    assert_eq(entry.data.len(), 4);

    // Error Display works
    let _ = hprintln!("test: Error Display...");
    let msg = alloc::format!("{}", sube::Error::BadInput);
    assert(!msg.is_empty());

    // Text type works on bare metal
    let _ = hprintln!("test: Text type...");
    let _text = sube::Text("(remark:0x68656c6c6f)");

    let _ = hprintln!("=== ALL PASSED ===");
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

fn assert(b: bool) {
    if !b {
        debug::exit(debug::EXIT_FAILURE);
    }
}
fn assert_eq(a: usize, b: usize) {
    assert(a == b);
}
