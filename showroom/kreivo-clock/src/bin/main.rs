//! Kreivo Clock — live blockchain data on your wrist
//!
//! Flash: espflash flash -p /dev/ttyACM0 -M target/xtensa-esp32s3-none-elf/release/kreivo-clock

#![no_std]
#![no_main]

extern crate alloc;
extern crate tinyrlibc;
use alloc::format;

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp_backtrace as _;

use kreivo_clock::board::Display;
use kreivo_clock::net::BlockEvent;

esp_bootloader_esp_idf::esp_app_desc!();

struct Ui {
    white: MonoTextStyle<'static, Rgb565>,
    green: MonoTextStyle<'static, Rgb565>,
    yellow: MonoTextStyle<'static, Rgb565>,
    dim: MonoTextStyle<'static, Rgb565>,
    red: MonoTextStyle<'static, Rgb565>,
}

impl Ui {
    fn new() -> Self {
        Self {
            white: MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE),
            green: MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_LIME_GREEN),
            yellow: MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_GOLD),
            dim: MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_DARK_GRAY),
            red: MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_ORANGE_RED),
        }
    }

    fn splash(&self, display: &mut Display) {
        let cyan = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
        display.clear(Rgb565::BLACK).ok();
        Text::new("kreivo", Point::new(75, 40), cyan).draw(display).ok();
        Text::new("clock", Point::new(88, 62), self.dim).draw(display).ok();
    }

    fn status(&self, display: &mut Display, msg: &str, style: MonoTextStyle<'_, Rgb565>) {
        clear_area(display, 0, 210, 240, 30);
        Text::new(msg, Point::new(20, 225), style).draw(display).ok();
    }
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let mut board = kreivo_clock::board::init(spawner).await;
    let ui = Ui::new();
    let d = &mut board.display;

    ui.splash(d);

    loop {
        // WiFi
        ui.status(d, "connecting wifi...", ui.dim);
        kreivo_clock::net::wifi_connect(&mut board.wifi).await;
        ui.status(d, "getting IP...", ui.dim);
        kreivo_clock::net::wait_for_ip(board.stack).await;
        ui.status(d, "wifi ok", ui.green);

        // Chain
        ui.status(d, "connecting...", ui.dim);
        let mut live = false;
        let result = kreivo_clock::net::watch_chain(board.stack, |event| {
            if !live {
                live = true;
                ui.status(d, "LIVE", ui.green);
                Text::new("LIVE", Point::new(95, 110), ui.green).draw(d).ok();
            }
            match event {
                BlockEvent::NewBlock { number } if number > 0 => {
                    let text = format!("#{number}");
                    clear_area(d, 20, 135, 200, 30);
                    Text::new(&text, Point::new(45, 155), ui.white).draw(d).ok();
                }
                BlockEvent::Finalized { count } => {
                    let text = format!("fin {count}");
                    clear_area(d, 20, 170, 200, 20);
                    Text::new(&text, Point::new(70, 185), ui.yellow).draw(d).ok();
                }
                _ => {}
            }
        })
        .await;

        if let Err(e) = result {
            log::error!("{e}");
            ui.status(d, "reconnecting...", ui.red);
            Timer::after(Duration::from_secs(3)).await;
        }

        let _ = board.wifi.disconnect_async().await;
        Timer::after(Duration::from_secs(1)).await;
    }
}

fn clear_area(display: &mut impl DrawTarget<Color = Rgb565>, x: i32, y: i32, w: u32, h: u32) {
    Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(display)
        .ok();
}
