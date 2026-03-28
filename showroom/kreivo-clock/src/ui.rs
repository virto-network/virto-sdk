//! Slint UI platform for T-Watch S3 (ST7789 240x240).

use alloc::rc::Rc;

use embedded_graphics_core::pixelcolor::raw::RawU16;
use embedded_graphics_core::pixelcolor::Rgb565;
use slint::platform::software_renderer::{
    LineBufferProvider, MinimalSoftwareWindow, Rgb565Pixel, RepaintBufferType,
};

use crate::board::{Display, DISPLAY_HEIGHT, DISPLAY_WIDTH};

/// Line buffer provider that pushes rendered lines to the ST7789 display.
pub struct DisplayBuffer<'a> {
    pub display: &'a mut Display,
    pub line_buf: &'a mut [Rgb565Pixel; DISPLAY_WIDTH],
}

impl LineBufferProvider for &mut DisplayBuffer<'_> {
    type TargetPixel = Rgb565Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Rgb565Pixel]),
    ) {
        let buf = &mut self.line_buf[range.clone()];
        render_fn(buf);
        self.display
            .set_pixels(
                range.start as u16,
                line as u16,
                range.end.saturating_sub(1) as u16,
                line as u16,
                buf.iter().map(|p| {
                    let raw: Rgb565 = RawU16::new(p.0).into();
                    raw
                }),
            )
            .ok();
    }
}

struct Platform {
    window: Rc<MinimalSoftwareWindow>,
}

impl slint::platform::Platform for Platform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> core::time::Duration {
        // embassy_time::Instant gives us monotonic time since boot
        let millis = embassy_time::Instant::now().as_millis();
        core::time::Duration::from_millis(millis)
    }
}

/// Initialize the Slint platform and return the window handle.
pub fn init() -> Rc<MinimalSoftwareWindow> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    window.set_size(slint::PhysicalSize::new(
        DISPLAY_WIDTH as u32,
        DISPLAY_HEIGHT as u32,
    ));
    slint::platform::set_platform(alloc::boxed::Box::new(Platform {
        window: window.clone(),
    }))
    .expect("slint platform");
    window
}
