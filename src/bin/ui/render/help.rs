use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};

use super::style::{BLACK, CYAN, MUTED, WHITE, text};

/// 1/ Cover the page with a short legend for the 5x5 keypad.
///
/// The full ASCII table is a later addition; this panel only explains how the
/// current hex-pair entry works.
pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(12, 32), Size::new(296, 180))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Rectangle::new(Point::new(12, 32), Size::new(296, 180))
        .into_styled(PrimitiveStyle::with_stroke(WHITE, 1))
        .draw(display)?;
    text(display, "KEYPAD HELP", 24, 42, CYAN, true)?;

    const LINES: [&str; 7] = [
        "0-F: HEX DIGITS, 2 = 1 ASCII CHAR",
        "41=A  5A=Z  61=a  7A=z  30=0",
        "20=SPACE  2E=.  2C=,  3F=?",
        "SEND / ENTER: CONFIRM ACTION",
        "BACK: DELETE, THEN LEAVE SCREEN",
        "ARROWS: SELECT OR MOVE CURSOR",
        "HELP: CLOSE THIS PANEL",
    ];
    for (index, line) in LINES.iter().enumerate() {
        text(display, line, 24, 70 + index as i32 * 18, WHITE, false)?;
    }

    text(display, "HELP / BACK / ENTER: CLOSE", 24, 196, MUTED, false)
}
