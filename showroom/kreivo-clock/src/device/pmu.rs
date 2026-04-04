//! AXP2101 PMU driver: power rails, battery monitoring, charging, and power key.

use esp_hal::i2c::master::I2c;

const AXP: u8 = 0x34;

/// AXP2101 PMU controller over I2C.
pub struct Pmu(I2c<'static, esp_hal::Blocking>);

impl Pmu {
    pub fn new(i2c: I2c<'static, esp_hal::Blocking>) -> Self {
        Self(i2c)
    }

    fn read_reg(&mut self, reg: u8) -> u8 {
        let mut b = [0u8; 1];
        let _ = self.0.write_read(AXP, &[reg], &mut b);
        b[0]
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

    /// Enable battery charger: 300mA, 4.2V target.
    fn enable_charging(&mut self) {
        // Reg 0x50: TS pin control — set to external input so it doesn't
        // block charging. Without this, the AXP2101 sees "temperature fault"
        // and refuses to charge. (From LILYGO XPowersLib: disableTSPinMeasure)
        let mut buf = [0u8; 1];
        if self.0.write_read(AXP, &[0x50], &mut buf).is_ok() {
            let _ = self.0.write(AXP, &[0x50, (buf[0] & 0xF0) | 0x10]);
        }

        // Reg 0x18: enable cell battery charging (bit 1), disable TS ADC (clear bit 1 of ADC)
        if self.0.write_read(AXP, &[0x18], &mut buf).is_ok() {
            let _ = self.0.write(AXP, &[0x18, buf[0] | 0x02]);
        }

        // Reg 0x62: charge current (ICC)
        // Bits 4:0 select current step. Values 8-16 map to 200-1000mA.
        // Step 10 = 300mA
        let _ = self.0.write(AXP, &[0x62, 10]);

        // Reg 0x64: charge target voltage (CV)
        // Bits 2:0: 000=4.0V, 001=4.1V, 010=4.2V, 011=4.35V, 100=4.4V
        let _ = self.0.write(AXP, &[0x64, 0x02]); // 4.2V

        // Reg 0x63: charge termination current + enable termination
        // Bit 4 = termination enable, bits 3:0 = current (25mA steps)
        // 0x10 = termination enabled, 25mA threshold
        let _ = self.0.write(AXP, &[0x63, 0x10]);

        // Reg 0x14: minimum system voltage = 4.5V
        let _ = self.0.write(AXP, &[0x14, 0x05]);

        log::info!(
            "PMU: charger cfg — 0x18={:#04x} 0x62={:#04x} 0x63={:#04x} 0x64={:#04x} 0x01={:#04x}",
            self.read_reg(0x18), self.read_reg(0x62), self.read_reg(0x63),
            self.read_reg(0x64), self.read_reg(0x01)
        );
    }

    /// Log charger register state (call after serial is ready).
    pub fn log_charger_state(&mut self) {
        let r00 = self.read_reg(0x00);
        let r01 = self.read_reg(0x01);
        let charge_state = r01 & 0x07;
        log::info!(
            "PMU: vbus={} charge_state={} (0=idle 1=pre 2=CC 3=CV 4=done 5=off)",
            r00 & 0x20 != 0,
            charge_state,
        );
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

    /// True if battery is currently charging.
    /// Register 0x01 bits 2:0 encode charge status.
    /// 001 = pre-charge, 010 = CC charge, 011 = CV charge
    pub fn is_charging(&mut self) -> bool {
        let mut buf = [0u8; 1];
        if self.0.write_read(AXP, &[0x01], &mut buf).is_ok() {
            let state = buf[0] & 0x07;
            state >= 1 && state <= 3
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
