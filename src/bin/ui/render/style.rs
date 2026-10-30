use core::fmt::Write;

use embedded_graphics::{
    Drawable,
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_6X10, FONT_9X18},
    },
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, RgbColor},
    text::{Baseline, Text},
};
use heapless::String;

pub const BLACK: Rgb565 = Rgb565::BLACK;
pub const WHITE: Rgb565 = Rgb565::WHITE;
pub const CYAN: Rgb565 = Rgb565::new(0, 50, 63);
pub const ACK_GREEN: Rgb565 = Rgb565::new(0, 63, 0);
pub const BLUE: Rgb565 = Rgb565::new(0, 20, 48);
pub const SELECTED: Rgb565 = Rgb565::new(8, 35, 50);
pub const MUTED: Rgb565 = Rgb565::new(35, 43, 52);
/// Unverified peers (heard on air, never paired) in the chats list.
pub const UNKNOWN: Rgb565 = Rgb565::new(60, 35, 0);
pub const WARNING: Rgb565 = Rgb565::new(31, 40, 0);

/// Draw one text line with the compact system font or the larger page-title font.
pub fn text<D>(
    display: &mut D,
    value: &str,
    x: i32,
    y: i32,
    color: Rgb565,
    large: bool,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let font = if large { FONT_9X18 } else { FONT_6X10 };
    Text::with_baseline(
        value,
        Point::new(x, y),
        MonoTextStyle::new(&font, color),
        Baseline::Top,
    )
    .draw(display)
    .map(|_| ())
}

/// Format signed microdegrees as `degrees.microseconds` for one chat or
/// navigation line, without allocating or depending on float formatting.
pub fn write_coordinate<const N: usize>(output: &mut String<N>, coordinate: i32) {
    let sign = if coordinate < 0 { "-" } else { "" };
    let absolute = coordinate.unsigned_abs();
    let _ = write!(
        output,
        "{sign}{}.{:06}",
        absolute / 1_000_000,
        absolute % 1_000_000
    );
}
