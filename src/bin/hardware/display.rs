//! Low-level SPI driver setup for the board's ILI9341 display.

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};

use embedded_hal::{delay::DelayNs, digital::OutputPin, spi::SpiDevice};
use mipidsi::{
    Builder,
    interface::SpiInterface,
    models::ILI9341Rgb565,
    options::{Orientation, Rotation},
};

/// Initializes the ILI9341 and returns its drawing target.
pub fn init<'a, SPI, DC, RESET, DELAY>(
    spi: SPI,
    dc: DC,
    reset: RESET,
    mut delay: DELAY,
    buffer: &'a mut [u8; 512],
) -> impl DrawTarget<Color = Rgb565> + 'a
where
    SPI: SpiDevice<u8> + 'a,
    DC: OutputPin + 'a,
    RESET: OutputPin + 'a,
    DELAY: DelayNs,
{
    let interface = SpiInterface::new(spi, dc, buffer);
    Builder::new(ILI9341Rgb565, interface)
        .reset_pin(reset)
        .orientation(Orientation::new().rotate(Rotation::Deg90))
        .init(&mut delay)
        .expect("Failed to initialize ILI9341 display")
}
