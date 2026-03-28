//! T-Watch S3 hardware: PMU, display, WiFi, network stack.

use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::timer::timg::TimerGroup;
use mipidsi::interface::SpiInterface;
use mipidsi::options::{ColorInversion, Orientation};
use static_cell::StaticCell;

const AXP: u8 = 0x34;
const SSID: &str = env!("WIFI_SSID");
const PASS: &str = env!("WIFI_PASS");

pub type Display = mipidsi::Display<
    SpiInterface<
        'static,
        ExclusiveDevice<
            Spi<'static, esp_hal::Blocking>,
            Output<'static>,
            embedded_hal_bus::spi::NoDelay,
        >,
        Output<'static>,
    >,
    mipidsi::models::ST7789,
    Output<'static>,
>;

pub struct Board {
    pub display: Display,
    pub wifi: esp_radio::wifi::WifiController<'static>,
    pub stack: embassy_net::Stack<'static>,
}

pub async fn init(spawner: Spawner) -> Board {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    esp_alloc::heap_allocator!(size: 196608);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    // --- PMU (AXP2101): enable all LDOs at 3.3V ---
    let mut i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(esp_hal::time::Rate::from_khz(100)),
    )
    .unwrap()
    .with_sda(peripherals.GPIO10)
    .with_scl(peripherals.GPIO11);
    let _ = i2c.write(AXP, &[0x90, 0xFF]); // enable all LDOs
    let _ = i2c.write(AXP, &[0x91, 0x01]); // DLDO1 enable
    for reg in 0x92..=0x9Au8 {
        let _ = i2c.write(AXP, &[reg, 0x1C]); // 3.3V
    }
    Timer::after(Duration::from_millis(50)).await;

    // --- Display (ST7789 240x240 via SPI) ---
    let _bl = Output::new(peripherals.GPIO45, Level::High, Default::default());
    // Leak backlight pin so it stays high for 'static
    core::mem::forget(_bl);

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
    static SPI_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    let spi_buf = SPI_BUF.init([0u8; 64]);
    let spi_iface = SpiInterface::new(spi_dev, dc, spi_buf);
    let rst = Output::new(peripherals.GPIO40, Level::High, Default::default());

    let display = mipidsi::Builder::new(mipidsi::models::ST7789, spi_iface)
        .display_size(240, 240)
        .orientation(Orientation::new())
        .invert_colors(ColorInversion::Inverted)
        .reset_pin(rst)
        .init(&mut embassy_time::Delay)
        .expect("display");

    // --- WiFi ---
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
    // TrngSource needs RNG + ADC1 for HW entropy (required by mbedtls CryptoRng)
    static TRNG_SRC: StaticCell<esp_hal::rng::TrngSource<'static>> = StaticCell::new();
    TRNG_SRC.init(esp_hal::rng::TrngSource::new(peripherals.RNG, peripherals.ADC1));

    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let seed = esp_hal::rng::Rng::new().random() as u64;
    let (stack, runner) = embassy_net::new(
        interfaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(net_task(runner)).ok();

    Board { display, wifi: controller, stack }
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, esp_radio::wifi::WifiDevice<'static>>) {
    runner.run().await;
}
