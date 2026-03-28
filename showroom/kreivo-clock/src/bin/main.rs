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

    // --- PMU ---
    let mut i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(esp_hal::time::Rate::from_khz(100)),
    )
    .unwrap()
    .with_sda(peripherals.GPIO10)
    .with_scl(peripherals.GPIO11);
    let _ = i2c.write(AXP, &[0x90, 0xFF]);
    let _ = i2c.write(AXP, &[0x91, 0x01]);
    for reg in 0x92..=0x9Au8 { let _ = i2c.write(AXP, &[reg, 0x1C]); }
    Timer::after(Duration::from_millis(50)).await;

    // --- Display ---
    let _bl = Output::new(peripherals.GPIO45, Level::High, Default::default());
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(esp_hal::time::Rate::from_mhz(40))
            .with_mode(SpiMode::_0),
    ).unwrap().with_sck(peripherals.GPIO18).with_mosi(peripherals.GPIO13);
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
    let red = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_ORANGE_RED);

    display.clear(Rgb565::BLACK).ok();
    Text::new("kreivo", Point::new(75, 40), cyan).draw(&mut display).ok();
    Text::new("clock", Point::new(88, 62), dim).draw(&mut display).ok();

    // --- WiFi ---
    draw_status(&mut display, "starting wifi...", dim);

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

    // --- Main loop: connect → watch → reconnect ---
    loop {
        // WiFi connect
        draw_status(&mut display, "connecting wifi...", dim);
        esp_println::println!("WiFi: connecting...");
        loop {
            match controller.connect_async().await {
                Ok(()) => break,
                Err(e) => {
                    esp_println::println!("WiFi: {:?}, retry in 5s", e);
                    draw_status(&mut display, "wifi retry...", red);
                    Timer::after(Duration::from_secs(5)).await;
                }
            }
        }
        esp_println::println!("WiFi: connected");

        // Wait for IP
        draw_status(&mut display, "getting IP...", dim);
        loop {
            if stack.is_config_up() { break; }
            Timer::after(Duration::from_millis(200)).await;
        }
        esp_println::println!("IP: {:?}", stack.config_v4().map(|c| c.address));
        draw_status(&mut display, "wifi ok", green);

        // TCP + TLS + WS + sube
        match connect_and_watch(stack, &mut display, white, yellow, green, dim, red).await {
            Ok(()) => {} // clean disconnect, loop back
            Err(e) => {
                esp_println::println!("Error: {}", e);
                draw_status(&mut display, "reconnecting...", red);
                Timer::after(Duration::from_secs(3)).await;
            }
        }

        // WiFi might have dropped — disconnect cleanly before retry
        let _ = controller.disconnect_async().await;
        Timer::after(Duration::from_secs(1)).await;
    }
}

async fn connect_and_watch(
    stack: embassy_net::Stack<'static>,
    display: &mut impl DrawTarget<Color = Rgb565>,
    white: MonoTextStyle<'_, Rgb565>,
    yellow: MonoTextStyle<'_, Rgb565>,
    green: MonoTextStyle<'_, Rgb565>,
    dim: MonoTextStyle<'_, Rgb565>,
    _red: MonoTextStyle<'_, Rgb565>,
) -> Result<(), &'static str> {
    // TCP
    draw_status(display, "connecting...", dim);
    let mut rx = [0u8; 4096];
    let mut tx = [0u8; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
    socket.set_timeout(Some(Duration::from_secs(15)));

    let remote = stack
        .dns_query("kreivo.io", embassy_net::dns::DnsQueryType::A)
        .await
        .map_err(|_| "DNS failed")?[0];

    socket.connect((remote, 443)).await.map_err(|_| "TCP failed")?;
    esp_println::println!("TCP connected");

    // TLS
    draw_status(display, "TLS...", dim);
    let mut tls_read_buf = alloc::vec![0u8; 16640];
    let mut tls_write_buf = alloc::vec![0u8; 16640];
    let tls_config = embedded_tls::TlsConfig::new()
        .with_server_name("kreivo.io");
    let mut tls = embedded_tls::TlsConnection::new(
        socket, &mut tls_read_buf, &mut tls_write_buf,
    );
    let mut crypto = EspCrypto(esp_hal::rng::Rng::new());
    tls.open(embedded_tls::TlsContext::new(&tls_config, &mut crypto))
        .await
        .map_err(|e| { esp_println::println!("TLS open error: {:?}", e); "TLS failed" })?;
    esp_println::println!("TLS handshake complete");

    // Consume any post-handshake messages (NewSessionTicket) before sending data
    // embedded-tls needs to process these before the connection is usable
    draw_status(display, "TLS ready...", dim);
    Timer::after(Duration::from_millis(500)).await;

    // WebSocket upgrade over TLS
    draw_status(display, "websocket...", dim);
    let ws = match sube::rpc::edge::Backend::connect(tls, "kreivo.io", "/").await {
        Ok(ws) => ws,
        Err(e) => {
            esp_println::println!("WS error: {e}");
            return Err("WS failed");
        }
    };
    esp_println::println!("WebSocket connected!");

    // ChainHead
    draw_status(display, "chain session...", dim);
    let mut chain = sube::rpc::chainhead::ChainHead::new(ws)
        .await
        .map_err(|e| { esp_println::println!("ChainHead error: {e}"); "ChainHead failed" })?;
    esp_println::println!("ChainHead started");

    draw_status(display, "LIVE", green);
    Text::new("LIVE", Point::new(95, 110), green).draw(display).ok();

    // Watch blocks
    loop {
        match chain.next_chain_event().await {
            Ok(sube::rpc::chainhead::ChainEvent::NewBlock { hash, .. }) => {
                if let Ok(header) = chain.header(&hash).await {
                    let text = format!("#{}", header.number);
                    clear_area(display, 20, 135, 200, 30);
                    Text::new(&text, Point::new(45, 155), white).draw(display).ok();
                }
            }
            Ok(sube::rpc::chainhead::ChainEvent::Finalized { hashes, .. }) => {
                let text = format!("fin {}", hashes.len());
                clear_area(display, 20, 170, 200, 20);
                Text::new(&text, Point::new(70, 185), yellow).draw(display).ok();
            }
            Ok(_) => {}
            Err(e) => {
                esp_println::println!("Chain error: {e}");
                return Err("chain disconnected");
            }
        }
    }
}

fn clear_area(display: &mut impl DrawTarget<Color = Rgb565>, x: i32, y: i32, w: u32, h: u32) {
    Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(display)
        .ok();
}

fn draw_status(display: &mut impl DrawTarget<Color = Rgb565>, msg: &str, style: MonoTextStyle<'_, Rgb565>) {
    clear_area(display, 0, 210, 240, 30);
    Text::new(msg, Point::new(20, 225), style).draw(display).ok();
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, esp_radio::wifi::WifiDevice<'static>>) {
    runner.run().await;
}

struct EspCrypto(esp_hal::rng::Rng);

impl embedded_tls::CryptoProvider for EspCrypto {
    type CipherSuite = embedded_tls::Aes128GcmSha256;
    type Signature = heapless::Vec<u8, 256>;

    fn rng(&mut self) -> impl rand_core_06::CryptoRng + rand_core_06::RngCore {
        EspRng06(&mut self.0)
    }
}

struct EspRng06<'a>(&'a mut esp_hal::rng::Rng);

impl rand_core_06::RngCore for EspRng06<'_> {
    fn next_u32(&mut self) -> u32 { self.0.random() }
    fn next_u64(&mut self) -> u64 { (self.0.random() as u64) << 32 | self.0.random() as u64 }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(4) {
            let r = self.0.random().to_le_bytes();
            chunk.copy_from_slice(&r[..chunk.len()]);
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core_06::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
impl rand_core_06::CryptoRng for EspRng06<'_> {}
