use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};

use crate::ui::state::ConversationKind;

use super::style::{BLUE, CYAN, MUTED, WHITE, text};

/// 1/ Draw the selected conversation, composer, context menu, and character grid.
pub fn draw<D>(
    display: &mut D,
    conversation: ConversationKind,
    input: &str,
    menu_open: bool,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let title = match conversation {
        ConversationKind::Broadcast => "BROADCAST [PUBLIC - UNENCRYPTED]",
        ConversationKind::Direct(_) => "DIRECT MESSAGE",
    };
    text(display, "<", 8, 34, CYAN, false)?;
    text(display, title, 28, 34, WHITE, false)?;
    Rectangle::new(Point::new(5, 54), Size::new(310, 91))
        .into_styled(PrimitiveStyle::with_stroke(MUTED, 1))
        .draw(display)?;
    text(
        display,
        "Message history will appear here",
        12,
        64,
        MUTED,
        false,
    )?;

    Rectangle::new(Point::new(5, 148), Size::new(310, 23))
        .into_styled(PrimitiveStyle::with_fill(BLUE))
        .draw(display)?;
    text(display, input, 10, 154, WHITE, false)?;
    text(display, "...", 294, 162, CYAN, false)?;

    if menu_open {
        draw_context_menu(display)
    } else {
        draw_character_grid(display)
    }
}

fn draw_context_menu<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(194, 92), Size::new(118, 52))
        .into_styled(PrimitiveStyle::with_fill(BLUE))
        .draw(display)?;
    text(display, "SEND COORDS", 200, 99, WHITE, false)?;
    text(display, "CANCEL", 200, 121, WHITE, false)
}

fn draw_character_grid<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    const KEYS: [&str; 5] = ["12345", "67890", "QWERT", "ASDFG", "ZXCVB"];
    for (row, keys) in KEYS.iter().enumerate() {
        for column in 0..keys.len() {
            text(
                display,
                &keys[column..column + 1],
                8 + column as i32 * 42,
                178 + row as i32 * 12,
                CYAN,
                false,
            )?;
        }
    }
    text(display, "<-  UP  DOWN  ->  ENT", 112, 218, WHITE, false)
}
