//! Kreivo Clock — live blockchain data on your wrist
//!
//! Core 0: WiFi + TLS + sube chain watcher (Embassy async)
//! Core 1: Slint UI render loop (dedicated, stutter-free)
//!
//! Flash: espflash flash -p /dev/ttyACM0 -M target/xtensa-esp32s3-none-elf/release/kreivo-clock

#![no_std]
#![no_main]

extern crate alloc;
extern crate tinyrlibc;

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::system::{CpuControl, Stack};
use heapless::spsc::Queue;
use slint::platform::software_renderer::Rgb565Pixel;
use static_cell::StaticCell;

use kreivo_clock::board::{self, DISPLAY_WIDTH};
use kreivo_clock::event::{Status, UiEvent};
use kreivo_clock::pmu::Pmu;
use kreivo_clock::ui::DisplayBuffer;

slint::include_modules!();
esp_bootloader_esp_idf::esp_app_desc!();

static EVENT_QUEUE: StaticCell<Queue<UiEvent, 16>> = StaticCell::new();
static APP_CORE_STACK: StaticCell<Stack<32768>> = StaticCell::new();

/// Battery percentage shared between cores (255 = unknown, 0-100 = valid).
static BATTERY_LEVEL: AtomicU8 = AtomicU8::new(255);
/// Set by PMU task on button press, consumed by UI core.
static SCREEN_TOGGLE: AtomicBool = AtomicBool::new(false);

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let system = kreivo_clock::board::init(spawner).await;

    // Let WiFi ppTask finish its late init before starting core 1
    Timer::after(Duration::from_millis(500)).await;

    let queue = EVENT_QUEUE.init(Queue::new());
    let (mut producer, consumer) = queue.split();

    // Start PMU polling task (battery + button)
    spawner.spawn(pmu_task(system.pmu)).ok();

    // Start core 1: UI render loop
    let mut cpu_control = CpuControl::new(system.cpu_ctrl);
    let stack = APP_CORE_STACK.init(Stack::new());
    let _guard = cpu_control
        .start_app_core(stack, move || {
            ui_core(consumer, system.display, system.backlight)
        })
        .expect("start core 1");

    // Core 0: chain watcher loop
    let mut wifi = system.wifi;
    let net_stack = system.stack;

    loop {
        kreivo_clock::net::wifi_connect(&mut wifi, &mut producer).await;
        kreivo_clock::net::wait_for_ip(net_stack, &mut producer).await;

        if let Err(e) = kreivo_clock::net::watch_chain(net_stack, &mut producer).await {
            log::error!("{e}");
            producer.enqueue(UiEvent::Live(false)).ok();
            producer.enqueue(UiEvent::Wifi(false)).ok();
            producer
                .enqueue(UiEvent::Status(Status::Error("reconnecting...")))
                .ok();
            Timer::after(Duration::from_secs(3)).await;
        }

        let _ = wifi.disconnect_async().await;
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// Format a block number with space-separated groups: 1234567 → "1 234 567".
fn format_block(n: u32) -> alloc::string::String {
    let s = alloc::format!("{n}");
    let len = s.len();
    let mut out = alloc::string::String::with_capacity(len + len / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Poll AXP2101 for battery level and power key presses.
#[embassy_executor::task]
async fn pmu_task(mut pmu: Pmu) {
    let mut tick = 0u32;
    loop {
        if pmu.button_pressed() {
            SCREEN_TOGGLE.store(true, Ordering::Relaxed);
        }

        // Read battery every ~30s (150 × 200ms)
        if tick % 150 == 0 {
            if let Some(pct) = pmu.battery_percent() {
                BATTERY_LEVEL.store(pct, Ordering::Relaxed);
            }
        }

        tick = tick.wrapping_add(1);
        Timer::after(Duration::from_millis(200)).await;
    }
}

/// Core 1 entry: owns display + Slint, renders in a tight loop.
fn ui_core(
    mut rx: heapless::spsc::Consumer<'static, UiEvent, 16>,
    mut display: board::Display,
    mut backlight: board::Backlight,
) -> ! {
    let window = kreivo_clock::ui::init();
    let app = MainWindow::new().expect("slint ui");
    let mut line_buf = [Rgb565Pixel(0); DISPLAY_WIDTH];
    let mut screen_on = true;

    loop {
        if SCREEN_TOGGLE.swap(false, Ordering::Relaxed) {
            screen_on = !screen_on;
            if screen_on {
                backlight.set_high();
            } else {
                backlight.set_low();
            }
        }

        while let Some(event) = rx.dequeue() {
            match event {
                UiEvent::Wifi(on) => app.set_wifi(on),
                UiEvent::Live(on) => app.set_live(on),
                UiEvent::Block(n) => {
                    app.set_block_number(n as i32);
                    app.set_block_text(format_block(n).into());
                }
                UiEvent::Collators(blocks) => {
                    app.set_c0(blocks[0] as i32);
                    app.set_c1(blocks[1] as i32);
                    app.set_c2(blocks[2] as i32);
                    app.set_c3(blocks[3] as i32);
                    app.set_c4(blocks[4] as i32);
                    app.set_c5(blocks[5] as i32);
                }
                UiEvent::Status(s) => {
                    let (msg, color) = match s {
                        Status::Dim(m) => (m, slint::Color::from_rgb_u8(100, 100, 100)),
                        Status::Good(m) => (m, slint::Color::from_rgb_u8(50, 205, 50)),
                        Status::Error(m) => (m, slint::Color::from_rgb_u8(255, 69, 0)),
                    };
                    app.set_status(msg.into());
                    app.set_status_color(color);
                }
            }
        }

        let batt = BATTERY_LEVEL.load(Ordering::Relaxed);
        app.set_battery_level(if batt <= 100 { batt as i32 } else { -1 });

        if screen_on {
            slint::platform::update_timers_and_animations();
            window.draw_if_needed(|renderer| {
                renderer.render_by_line(&mut DisplayBuffer {
                    display: &mut display,
                    line_buf: &mut line_buf,
                });
            });
        }
    }
}
