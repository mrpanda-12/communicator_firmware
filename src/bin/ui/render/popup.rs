use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};
use heapless::String;

use crate::notify::{Popup, PopupKind};
use crate::storage::message::ConversationId;

use super::style::{CYAN, WHITE, text};

/// Draw one received-message toast over whatever screen is active.
pub fn draw<D>(display: &mut D, popup: &Popup) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(90, 36), Size::new(140, 44))
        .into_styled(PrimitiveStyle::with_fill(super::style::BLUE))
        .draw(display)?;
    Rectangle::new(Point::new(90, 36), Size::new(140, 44))
        .into_styled(PrimitiveStyle::with_stroke(super::style::CYAN, 1))
        .draw(display)?;

    let mut title = String::<32>::new();
    let _ = match (popup.kind, popup.conversation) {
        (PopupKind::Alert, _) => title.push_str("ALERT"),
        (PopupKind::NewMessage, ConversationId::Broadcast) => title.push_str("BROADCAST"),
        (PopupKind::NewMessage, ConversationId::Direct(_)) => title.push_str("NEW MESSAGE"),
    };
    text(display, title.as_str(), 100, 44, WHITE, false)?;
    text(display, popup.text.as_str(), 100, 62, CYAN, false)
}
