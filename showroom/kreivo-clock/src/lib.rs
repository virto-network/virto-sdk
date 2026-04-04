#![no_std]
extern crate alloc;

pub mod device;
pub mod flash;
pub mod http;

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::system::{CpuControl, Stack};
use heapless::spsc::{Producer, Queue};
use slint::platform::software_renderer::Rgb565Pixel;
use static_cell::StaticCell;

use device::board::{self, DISPLAY_WIDTH};
use device::event::{Status, UiEvent};
use device::ui::DisplayBuffer;

slint::include_modules!();

static EVENT_QUEUE: StaticCell<Queue<UiEvent, 16>> = StaticCell::new();
static APP_CORE_STACK: StaticCell<Stack<32768>> = StaticCell::new();

/// Battery percentage shared between cores (255 = unknown, 0-100 = valid).
static BATTERY_LEVEL: AtomicU8 = AtomicU8::new(255);
/// Set by PMU task on button press, consumed by UI core.
static SCREEN_TOGGLE: AtomicBool = AtomicBool::new(false);
/// WiFi connection status, set by wifi_task, read by UI core.
static WIFI_CONNECTED: AtomicBool = AtomicBool::new(false);
/// Battery charging status, set by pmu_task, read by UI core.
static CHARGING: AtomicBool = AtomicBool::new(false);
/// Screen state — written by UI core, read by chain loop to skip queries when off.
pub static SCREEN_ON: AtomicBool = AtomicBool::new(true);

/// What the app needs after device init.
pub struct Runtime {
    pub stack: embassy_net::Stack<'static>,
    pub events: Producer<'static, UiEvent, 16>,
    /// Pre-allocated registry for metadata loading.
    /// Allocated early (before WiFi) from clean, unfragmented heap.
    pub registry: sube::Registry,
}

/// Initialize hardware, start background tasks (WiFi, PMU), and UI core.
///
/// WiFi connects automatically in the background and reconnects on failure.
/// Returns once WiFi has an IP address so the app can start immediately.
pub async fn start(spawner: Spawner) -> Runtime {
    let (system, registry) = device::board::init(spawner).await;

    // Let WiFi ppTask finish its late init before starting core 1
    Timer::after(Duration::from_millis(500)).await;

    let queue = EVENT_QUEUE.init(Queue::new());
    let (producer, consumer) = queue.split();

    // Start PMU polling task (battery + button)
    spawner.spawn(pmu_task(system.pmu)).ok();

    // Start WiFi management task (connect + auto-reconnect)
    spawner.spawn(wifi_task(system.wifi)).ok();

    // Start core 1: UI render loop
    let mut cpu_control = CpuControl::new(system.cpu_ctrl);
    let stack = APP_CORE_STACK.init(Stack::new());
    let _guard = cpu_control
        .start_app_core(stack, move || {
            ui_core(consumer, system.display, system.backlight)
        })
        .expect("start core 1");
    core::mem::forget(_guard);

    // Wait for WiFi + IP before returning
    log::info!("WiFi: waiting for connection...");
    loop {
        if system.stack.is_config_up() {
            break;
        }
        Timer::after(Duration::from_millis(200)).await;
    }
    log::info!("IP: {:?}", system.stack.config_v4().map(|c| c.address));

    Runtime {
        stack: system.stack,
        events: producer,
        registry,
    }
}

/// Background task: keep WiFi connected, reconnect on failure.
#[embassy_executor::task]
async fn wifi_task(mut wifi: esp_radio::wifi::WifiController<'static>) {
    loop {
        log::info!("WiFi: connecting...");
        match wifi.connect_async().await {
            Ok(()) => {
                log::info!("WiFi: connected");
                WIFI_CONNECTED.store(true, Ordering::Relaxed);
                wifi.wait_for_event(esp_radio::wifi::WifiEvent::StaDisconnected)
                    .await;
                WIFI_CONNECTED.store(false, Ordering::Relaxed);
                log::warn!("WiFi: disconnected, reconnecting in 1s");
            }
            Err(e) => {
                log::warn!("WiFi: connect failed: {:?}, retry in 5s", e);
                Timer::after(Duration::from_secs(5)).await;
                continue;
            }
        }
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// Format a block number with space-separated groups: 1234567 → "1 234 567".
pub fn format_block(n: u32) -> alloc::string::String {
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
async fn pmu_task(mut pmu: device::pmu::Pmu) {
    let mut tick = 0u32;
    loop {
        if pmu.button_pressed() {
            SCREEN_TOGGLE.store(true, Ordering::Relaxed);
        }

        // Log charger state ~1s after boot
        if tick == 5 {
            pmu.log_charger_state();
        }

        // Read battery + charge status every ~30s (150 × 200ms)
        if tick % 150 == 0 {
            if let Some(pct) = pmu.battery_percent() {
                BATTERY_LEVEL.store(pct, Ordering::Relaxed);
            }
            CHARGING.store(pmu.is_charging(), Ordering::Relaxed);
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
    let window = device::ui::init();
    let app = MainWindow::new().expect("slint ui");
    let mut line_buf = [Rgb565Pixel(0); DISPLAY_WIDTH];
    let mut screen_on = true;
    let mut loading_frame = 0u32;
    let mut last_activity = esp_hal::time::Instant::now();
    let screen_timeout = esp_hal::time::Duration::from_secs(15);

    loop {
        if SCREEN_TOGGLE.swap(false, Ordering::Relaxed) {
            if !screen_on {
                // Wake up — turn screen back on
                screen_on = true;
                backlight.set_high();
                SCREEN_ON.store(true, Ordering::Relaxed);
            } else {
                // Toggle between main view and info page
                let showing = app.get_show_info();
                app.set_show_info(!showing);
                if !showing {
                    app.set_heap_free((esp_alloc::HEAP.free() / 1024) as i32);
                }
            }
            last_activity = esp_hal::time::Instant::now();
        }

        // Auto screen off after 15s idle
        if screen_on && last_activity.elapsed() > screen_timeout {
            screen_on = false;
            backlight.set_low();
            SCREEN_ON.store(false, Ordering::Relaxed);
        }

        // Loading animation: cycle segments 0-7 while not live (~200ms per step)
        if !app.get_live() {
            loading_frame = loading_frame.wrapping_add(1);
            app.set_loading_step((loading_frame / 100 % 8) as i32);
        } else {
            app.set_loading_step(-1);
        }

        while let Some(event) = rx.dequeue() {
            if screen_on {
                last_activity = esp_hal::time::Instant::now();
            }
            match event {
                UiEvent::Live(on) => app.set_live(on),
                UiEvent::Block(n) => {
                    app.set_block_number(n as i32);
                    app.set_block_text(format_block(n).into());
                }
                UiEvent::Collators(blocks) => {
                    let current = app.get_block_number() as u32;
                    let fmt = |b: u32| -> slint::SharedString {
                        if b > 0 {
                            format_block(b).into()
                        } else {
                            "".into()
                        }
                    };
                    let active = |b: u32| -> bool { b > 0 && current.saturating_sub(b) < 100 };
                    app.set_c0(fmt(blocks[0]));
                    app.set_c0_active(active(blocks[0]));
                    app.set_c1(fmt(blocks[1]));
                    app.set_c1_active(active(blocks[1]));
                    app.set_c2(fmt(blocks[2]));
                    app.set_c2_active(active(blocks[2]));
                    app.set_c3(fmt(blocks[3]));
                    app.set_c3_active(active(blocks[3]));
                    app.set_c4(fmt(blocks[4]));
                    app.set_c4_active(active(blocks[4]));
                    app.set_c5(fmt(blocks[5]));
                    app.set_c5_active(active(blocks[5]));
                }
                UiEvent::ConfigUrl(url) => {
                    let s: slint::SharedString = url.as_str().into();
                    app.set_config_url(s);
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

        if screen_on {
            app.set_wifi(WIFI_CONNECTED.load(Ordering::Relaxed));
            app.set_charging(CHARGING.load(Ordering::Relaxed));
            let batt = BATTERY_LEVEL.load(Ordering::Relaxed);
            app.set_battery_level(if batt <= 100 { batt as i32 } else { -1 });
            app.set_meta_pallets(http::PALLET_COUNT.load(Ordering::Relaxed) as i32);
            app.set_meta_saved(http::META_SAVED.load(Ordering::Relaxed));
            slint::platform::update_timers_and_animations();
            window.draw_if_needed(|renderer| {
                renderer.render_by_line(&mut DisplayBuffer {
                    display: &mut display,
                    line_buf: &mut line_buf,
                });
            });
        }

        // Low power: when screen is off, sleep core 1.
        // Only wake to check button press (SCREEN_TOGGLE) every ~200ms.
        if !screen_on {
            // Use waiti to put core 1 in idle — wakes on any interrupt.
            // Fall back to a delay loop (~200ms at 240MHz).
            for _ in 0..10_000_000u32 {
                core::hint::spin_loop();
            }
        }
    }
}
