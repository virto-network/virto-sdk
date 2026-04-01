//! AXP2101 PMU driver: power rails, battery monitoring, and power key.

use esp_hal::i2c::master::I2c;

const AXP: u8 = 0x34;

/// AXP2101 PMU controller over I2C.
pub struct Pmu(I2c<'static, esp_hal::Blocking>);

impl Pmu {
    pub fn new(i2c: I2c<'static, esp_hal::Blocking>) -> Self {
        Self(i2c)
    }

    /// Enable all LDOs at 3.3V for T-Watch S3 peripherals.
    pub fn enable_power(&mut self) {
        let _ = self.0.write(AXP, &[0x90, 0xFF]);
        let _ = self.0.write(AXP, &[0x91, 0x01]);
        for reg in 0x92..=0x9Au8 {
            let _ = self.0.write(AXP, &[reg, 0x1C]);
        }
        self.enable_charging();
    }

    /// Enable battery charger at 300mA, 4.2V target.
    fn enable_charging(&mut self) {
        // Register 0x62: Linear charger control
        // Bit 0 = charger enable, bits 4-0 = charge current
        // 0x04 = enable + 300mA (safe default for small LiPo)
        let _ = self.0.write(AXP, &[0x62, 0x04]);
        // Register 0x63: Charge target voltage = 4.2V (bits 2-0 = 0b010)
        let _ = self.0.write(AXP, &[0x63, 0x02]);
        // Register 0x14: minimum system voltage = 4.5V (ensures USB powers device while charging)
        let _ = self.0.write(AXP, &[0x14, 0x05]);
        log::info!("PMU: charger enabled (300mA, 4.2V)");
    }

    /// Enable battery voltage ADC and clear stale IRQs from boot.
    pub fn enable_monitoring(&mut self) {
        let mut buf = [0u8; 1];
        if self.0.write_read(AXP, &[0x18], &mut buf).is_ok() {
            let _ = self.0.write(AXP, &[0x18, buf[0] | 0x0F]);
        }
        for &reg in &[0x48u8, 0x49, 0x4A] {
            let _ = self.0.write(AXP, &[reg, 0xFF]);
        }
    }

    /// Battery percentage from VBAT ADC (registers 0x34/0x35, 1mV/LSB).
    pub fn battery_percent(&mut self) -> Option<u8> {
        let mut buf = [0u8; 2];
        self.0.write_read(AXP, &[0x34], &mut buf).ok()?;
        let mv = ((buf[0] as u16 & 0x3F) << 8) | buf[1] as u16;
        Some(match mv {
            0..=3000 => 0,
            3001..=4200 => ((mv as u32 - 3000) * 100 / 1200) as u8,
            _ => 100,
        })
    }

    /// True if battery is currently charging (register 0x01, bit 2).
    pub fn is_charging(&mut self) -> bool {
        let mut buf = [0u8; 1];
        if self.0.write_read(AXP, &[0x01], &mut buf).is_ok() {
            buf[0] & 0x04 != 0
        } else {
            false
        }
    }

    /// Check and clear power key short press (INTSTS2 register 0x49, bit 1).
    pub fn button_pressed(&mut self) -> bool {
        let mut buf = [0u8; 1];
        if self.0.write_read(AXP, &[0x49], &mut buf).is_ok() && buf[0] & 0x02 != 0 {
            let _ = self.0.write(AXP, &[0x49, buf[0]]);
            true
        } else {
            false
        }
    }
}
