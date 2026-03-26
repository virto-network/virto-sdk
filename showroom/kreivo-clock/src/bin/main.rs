//! Kreivo Clock — live blockchain data on your wrist
//!
//! Flash: espflash flash -p /dev/ttyACM0 -M target/xtensa-esp32s3-none-elf/release/kreivo-clock

#![no_std]
#![no_main]
#![allow(clippy::mem_forget, clippy::large_stack_frames)]

extern crate alloc;
use alloc::format;

use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_hal::i2c::I2c as _;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::timer::timg::TimerGroup;
use mipidsi::interface::SpiInterface;
use mipidsi::options::{ColorInversion, Orientation};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const AXP: u8 = 0x34;
const SSID: &str = env!("WIFI_SSID");
const PASS: &str = env!("WIFI_PASS");

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    esp_alloc::heap_allocator!(size: 196608);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    // --- PMU: power on display ---
    let mut i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(esp_hal::time::Rate::from_khz(100)),
    )
    .unwrap()
    .with_sda(peripherals.GPIO10)
    .with_scl(peripherals.GPIO11);

    let _ = i2c.write(AXP, &[0x90, 0xFF]);
    let _ = i2c.write(AXP, &[0x91, 0x01]);
    for reg in 0x92..=0x9Au8 {
        let _ = i2c.write(AXP, &[reg, 0x1C]);
    }
    Timer::after(Duration::from_millis(50)).await;

    // --- Display ---
    let _bl = Output::new(peripherals.GPIO45, Level::High, Default::default());
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(esp_hal::time::Rate::from_mhz(40))
            .with_mode(SpiMode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO18)
    .with_mosi(peripherals.GPIO13);

    let dc = Output::new(peripherals.GPIO38, Level::Low, Default::default());
    let cs = Output::new(peripherals.GPIO12, Level::High, Default::default());
    let spi_dev = ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let mut spi_buf = [0u8; 64];
    let spi_iface = SpiInterface::new(spi_dev, dc, &mut spi_buf);
    let rst = Output::new(peripherals.GPIO40, Level::High, Default::default());

    let mut display = mipidsi::Builder::new(mipidsi::models::ST7789, spi_iface)
        .display_size(240, 240)
        .orientation(Orientation::new())
        .invert_colors(ColorInversion::Inverted)
        .reset_pin(rst)
        .init(&mut embassy_time::Delay)
        .expect("display");

    let cyan = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let white = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let green = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_LIME_GREEN);
    let dim = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_DARK_GRAY);
    let yellow = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_GOLD);

    display.clear(Rgb565::BLACK).ok();
    Text::new("kreivo", Point::new(75, 40), cyan).draw(&mut display).ok();
    Text::new("clock", Point::new(88, 62), dim).draw(&mut display).ok();

    // --- WiFi ---
    draw_status(&mut display, "connecting wifi...", dim);
    esp_println::println!("WiFi: connecting to {SSID}");

    static RADIO: StaticCell<esp_radio::Controller<'static>> = StaticCell::new();
    let radio_init = RADIO.init(esp_radio::init().expect("radio"));
    let (mut controller, interfaces) =
        esp_radio::wifi::new(radio_init, peripherals.WIFI, Default::default()).expect("wifi");

    controller
        .set_config(&esp_radio::wifi::ModeConfig::Client(
            esp_radio::wifi::ClientConfig::default()
                .with_ssid(SSID.try_into().unwrap())
                .with_password(PASS.try_into().unwrap()),
        ))
        .unwrap();
    controller.start_async().await.unwrap();
    controller.connect_async().await.unwrap();

    draw_status(&mut display, "wifi connected", dim);
    esp_println::println!("WiFi: connected");

    // --- Network stack ---
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let seed = esp_hal::rng::Rng::new().random() as u64;
    let (stack, runner) = embassy_net::new(
        interfaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(net_task(runner)).ok();

    // Wait for IP
    loop {
        if stack.is_config_up() { break; }
        Timer::after(Duration::from_millis(100)).await;
    }
    let ip = stack.config_v4().map(|c| format!("{}", c.address.address()));
    esp_println::println!("IP: {:?}", ip);
    draw_status(&mut display, "got IP", dim);

    // --- TCP + WebSocket to Kreivo ---
    draw_status(&mut display, "connecting kreivo...", dim);

    let mut rx = [0u8; 4096];
    let mut tx = [0u8; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
    socket.set_timeout(Some(Duration::from_secs(30)));

    // DNS resolve
    let remote = stack
        .dns_query("kreivo.io", embassy_net::dns::DnsQueryType::A)
        .await
        .expect("DNS")[0];

    // Note: kreivo.io requires TLS (port 443). Without TLS this will fail
    // the WebSocket upgrade. For now connect to port 443 and try.
    socket.connect((remote, 443)).await.expect("TCP");
    esp_println::println!("TCP connected");

    // WebSocket upgrade
    let ws = sube::rpc::edge::Backend::connect(socket, "kreivo.io", "/")
        .await
        .expect("WS");
    esp_println::println!("WebSocket connected");
    draw_status(&mut display, "chain connected!", green);

    // --- ChainHead ---
    let mut chain = sube::rpc::chainhead::ChainHead::new(ws).await.expect("ChainHead");
    esp_println::println!("ChainHead started");

    Text::new("LIVE", Point::new(95, 110), green).draw(&mut display).ok();

    // --- Watch blocks ---
    loop {
        match chain.next_chain_event().await {
            Ok(sube::rpc::chainhead::ChainEvent::NewBlock { hash, .. }) => {
                if let Ok(header) = chain.header(&hash).await {
                    let text = format!("#{}", header.number);
                    Rectangle::new(Point::new(20, 135), Size::new(200, 30))
                        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
                        .draw(&mut display)
                        .ok();
                    Text::new(&text, Point::new(45, 155), white)
                        .draw(&mut display)
                        .ok();
                    esp_println::println!("{text}");
                }
            }
            Ok(sube::rpc::chainhead::ChainEvent::Finalized { hashes, .. }) => {
                let text = format!("fin {}", hashes.len());
                Rectangle::new(Point::new(20, 170), Size::new(200, 20))
                    .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
                    .draw(&mut display)
                    .ok();
                Text::new(&text, Point::new(70, 185), yellow)
                    .draw(&mut display)
                    .ok();
            }
            Ok(_) => {}
            Err(e) => {
                esp_println::println!("error: {e}");
                draw_status(&mut display, "reconnecting...", dim);
                Timer::after(Duration::from_secs(2)).await;
            }
        }
    }
}

fn draw_status(
    display: &mut impl DrawTarget<Color = Rgb565>,
    msg: &str,
    style: MonoTextStyle<'_, Rgb565>,
) {
    Rectangle::new(Point::new(0, 210), Size::new(240, 30))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(display)
        .ok();
    Text::new(msg, Point::new(20, 225), style).draw(display).ok();
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, esp_radio::wifi::WifiDevice<'static>>) {
    runner.run().await;
}
