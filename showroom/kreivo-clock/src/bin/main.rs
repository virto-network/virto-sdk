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

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::system::{CpuControl, Stack};
use heapless::spsc::Queue;
use slint::platform::software_renderer::Rgb565Pixel;
use static_cell::StaticCell;

use kreivo_clock::board::DISPLAY_WIDTH;
use kreivo_clock::event::{Status, UiEvent};
use kreivo_clock::ui::DisplayBuffer;

slint::include_modules!();
esp_bootloader_esp_idf::esp_app_desc!();

static EVENT_QUEUE: StaticCell<Queue<UiEvent, 16>> = StaticCell::new();
static APP_CORE_STACK: StaticCell<Stack<32768>> = StaticCell::new();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let system = kreivo_clock::board::init(spawner).await;
    log::info!("Board init complete");

    // Let WiFi ppTask finish its late init before starting core 1
    Timer::after(Duration::from_millis(500)).await;

    let queue = EVENT_QUEUE.init(Queue::new());
    let (mut producer, consumer) = queue.split();

    // Start core 1: UI render loop
    let display = system.display;
    let mut cpu_control = CpuControl::new(system.cpu_ctrl);
    let stack = APP_CORE_STACK.init(Stack::new());
    let _guard = cpu_control
        .start_app_core(stack, move || ui_core(consumer, display))
        .expect("start core 1");

    log::info!("Core 1 started");

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
            producer.enqueue(UiEvent::Status(Status::Error("reconnecting..."))).ok();
            Timer::after(Duration::from_secs(3)).await;
        }

        let _ = wifi.disconnect_async().await;
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// Core 1 entry: owns display + Slint, renders in a tight loop.
fn ui_core(
    mut rx: heapless::spsc::Consumer<'static, UiEvent, 16>,
    mut display: kreivo_clock::board::Display,
) -> ! {
    let window = kreivo_clock::ui::init();
    let app = MainWindow::new().expect("slint ui");
    let mut line_buf = [Rgb565Pixel(0); DISPLAY_WIDTH];

    loop {
        while let Some(event) = rx.dequeue() {
            match event {
                UiEvent::Wifi(on) => app.set_wifi(on),
                UiEvent::Live(on) => app.set_live(on),
                UiEvent::Block(n) => app.set_block_number(n as i32),
                UiEvent::Finalized(n) => app.set_finalized_count(n as i32),
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

        slint::platform::update_timers_and_animations();
        window.draw_if_needed(|renderer| {
            renderer.render_by_line(&mut DisplayBuffer {
                display: &mut display,
                line_buf: &mut line_buf,
            });
        });
    }
}
