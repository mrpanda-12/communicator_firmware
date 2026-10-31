use core::fmt::Write as _;

use heapless::String;

use crate::notify::{Popup, PopupType};
use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};

use super::screens::settings::frequency_label;
use super::style::{BLUE, CYAN, WHITE, text};

/// Draw one notification over the active screen. ENTER accepts or acknowledges
/// it, BACK dismisses it, and every toast says so on its own footer.
pub fn draw<D>(display: &mut D, popup: &Popup) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let request = match popup.popup_type {
        PopupType::FrequencyChangeRequest { frequency_hz } => Some(frequency_hz),
        _ => None,
    };
    Rectangle::new(Point::new(10, 36), Size::new(300, 64))
        .into_styled(PrimitiveStyle::with_fill(BLUE))
        .draw(display)?;
    Rectangle::new(Point::new(10, 36), Size::new(300, 64))
        .into_styled(PrimitiveStyle::with_stroke(CYAN, 1))
        .draw(display)?;

    match popup.popup_type {
        PopupType::Alert => text(display, "ALERT", 20, 44, WHITE, false)?,
        PopupType::ContactRequest => text(display, popup.name.as_str(), 20, 44, WHITE, false)?,
        PopupType::Text | PopupType::Gps => {
            text(display, popup.name.as_str(), 20, 44, WHITE, false)?;
            if popup.conversation.is_broadcast() {
                text(display, "BROADCAST", 240, 44, CYAN, false)?;
            } else if matches!(popup.popup_type, PopupType::Gps) {
                text(display, "GPS", 282, 44, CYAN, false)?;
            }
        }
        PopupType::FrequencyChangeRequest { frequency_hz } => {
            text(display, popup.name.as_str(), 20, 44, WHITE, false)?;
            let mut line = String::<40>::new();
            let _ = write!(line, "WANTS {}", frequency_label(frequency_hz).as_str());
            text(display, line.as_str(), 20, 62, CYAN, false)?;
        }
    }

    match request {
        Some(_) => text(display, "ENTER: ACCEPT  BACK: NO", 20, 82, WHITE, false),
        None => {
            text(display, popup.text.as_str(), 20, 62, CYAN, false)?;
            text(display, "ENTER / BACK: CLOSE", 20, 82, WHITE, false)
        }
    }
}
