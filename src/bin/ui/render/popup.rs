use crate::notify::{Popup, PopupType};
use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};

use super::style::{CYAN, WHITE, text};

/// Draw one received-message toast over whatever screen is active.
pub fn draw<D>(display: &mut D, popup: &Popup) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(10, 36), Size::new(300, 44))
        .into_styled(PrimitiveStyle::with_fill(super::style::BLUE))
        .draw(display)?;
    Rectangle::new(Point::new(10, 36), Size::new(300, 44))
        .into_styled(PrimitiveStyle::with_stroke(super::style::CYAN, 1))
        .draw(display)?;

    // Top row: who it came from on the left, the public marker on the right.
    match popup.popup_type {
        PopupType::Alert => text(display, "ALERT", 20, 44, WHITE, false)?,
        PopupType::ContactRequest => text(display, popup.name.as_str(), 20, 44, WHITE, false)?,
        PopupType::NewMessage => {
            text(display, popup.name.as_str(), 20, 44, WHITE, false)?;
            if popup.conversation.is_broadcast() {
                text(display, "BROADCAST", 246, 44, CYAN, false)?;
            }
        }
    }
    text(display, popup.text.as_str(), 20, 62, CYAN, false)
}
