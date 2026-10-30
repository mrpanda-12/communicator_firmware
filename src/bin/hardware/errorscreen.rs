//! Startup error page for a boot failure that the display can still report.
//!
//! Every failure before the panel exists (flash range, TRNG, display bus or
//! panel) can only be logged, but once the ILI9341 draws, the reason belongs on
//! the board instead of in the serial log, followed by a restart.

use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_hal::delay::DelayNs;

use crate::ui::render::style::{BLACK, MUTED, WARNING, WHITE, text};

/// Seconds the reason stays visible before the board restarts itself.
const REBOOT_SECONDS: u8 = 10;
/// Left edge of every line, clear of the bezel.
const MARGIN: i32 = 20;

/// Show the reason, count down, then reboot. This never returns.
pub fn show<D>(display: &mut D, reason: &str) -> !
where
    D: DrawTarget<Color = Rgb565>,
{
    defmt::error!(
        "Startup error: {}; rebooting in {} s",
        reason,
        REBOOT_SECONDS
    );
    let _ = page(display, reason);

    let mut delay = embassy_time::Delay;
    for remaining in (0..=REBOOT_SECONDS).rev() {
        let _ = countdown(display, remaining);
        if remaining == 0 {
            break;
        }
        delay.delay_ms(1_000);
    }
    esp_hal::system::software_reset()
}

/// Draw the static part once: background, heading, reason, and the notice.
fn page<D>(display: &mut D, reason: &str) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::zero(), Size::new(320, 240))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    text(display, "STARTUP ERROR", MARGIN, 56, WARNING, true)?;
    text(display, reason, MARGIN, 104, WHITE, false)?;
    text(
        display,
        "THE DEVICE RESTARTS BY ITSELF.",
        MARGIN,
        140,
        MUTED,
        false,
    )
}

/// Repaint only the counter line, so each second costs one small SPI write.
fn countdown<D>(display: &mut D, remaining: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(MARGIN - 4, 168), Size::new(280, 20))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    let mut line = heapless::String::<24>::new();
    let _ = core::fmt::Write::write_fmt(&mut line, format_args!("REBOOTING IN {remaining:>2} S"));
    text(display, line.as_str(), MARGIN, 172, WARNING, false)
}
