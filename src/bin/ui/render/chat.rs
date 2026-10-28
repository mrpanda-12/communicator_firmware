use core::fmt::Write as _;

use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};
use heapless::String;

use crate::storage::message::ConversationId;
use crate::ui::state::ChatRow;

use super::style::{ACK_GREEN, BLUE, CYAN, MUTED, WARNING, WHITE, text};

/// 1/ Draw the selected conversation, composer, and context menu.
pub fn draw<D>(
    display: &mut D,
    conversation: ConversationId,
    rows: &[ChatRow],
    input: &str,
    pending_hex: Option<u8>,
    menu_open: bool,
    unencrypted_dm: bool,
    send_enabled: bool,
    selected_message: Option<usize>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_header(display, conversation, unencrypted_dm)?;
    draw_message_panel(display, conversation, rows, selected_message)?;
    draw_input(display, input, pending_hex)?;
    draw_chat_controls(display, send_enabled)?;

    if menu_open {
        draw_context_menu(display)
    } else {
        Ok(())
    }
}

/// 2/ Repaint only the chat title row when the active conversation changes.
pub fn draw_header<D>(
    display: &mut D,
    conversation: ConversationId,
    unencrypted_dm: bool,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let title = match (conversation, unencrypted_dm) {
        (ConversationId::Broadcast, _) => "BROADCAST [PUBLIC - UNENCRYPTED]",
        (ConversationId::Direct(_), true) => "DIRECT [UNENCRYPTED]",
        (ConversationId::Direct(_), false) => "DIRECT MESSAGE",
    };
    Rectangle::new(Point::new(5, 28), Size::new(310, 22))
        .into_styled(PrimitiveStyle::with_fill(super::style::BLACK))
        .draw(display)?;
    text(display, "<", 8, 34, CYAN, false)?;
    text(display, title, 28, 34, WHITE, false)
}

/// 3/ Draw up to four history rows. Encryption tints them cyan; a sent row of
/// an encrypted chat reads green once its ACK arrived, grey with SENDING.. before.
pub fn draw_message_panel<D>(
    display: &mut D,
    conversation: ConversationId,
    rows: &[ChatRow],
    selected_message: Option<usize>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    // Clear the interior first so a shorter list never leaves older text behind.
    Rectangle::new(Point::new(6, 55), Size::new(308, 89))
        .into_styled(PrimitiveStyle::with_fill(super::style::BLACK))
        .draw(display)?;
    Rectangle::new(Point::new(5, 54), Size::new(310, 91))
        .into_styled(PrimitiveStyle::with_stroke(MUTED, 1))
        .draw(display)?;
    if rows.is_empty() {
        text(display, "NO MESSAGES YET", 24, 94, MUTED, false)?;
    }
    for (index, row) in rows.iter().enumerate() {
        let row_y = 59 + index as i32 * 19;
        if Some(index) == selected_message {
            Rectangle::new(Point::new(7, row_y), Size::new(306, 17))
                .into_styled(PrimitiveStyle::with_fill(super::style::SELECTED))
                .draw(display)?;
        }
        let mut line = String::<48>::new();
        let label = if row.coordinates.is_some() {
            if row.from_me { "GPS->" } else { "GPS<-" }
        } else if row.from_me {
            "ME>"
        } else {
            "FROM"
        };
        let _ = write!(line, "{label} ");
        if let Some((latitude, longitude)) = row.coordinates {
            write_coordinate(&mut line, latitude);
            let _ = line.push(',');
            write_coordinate(&mut line, longitude);
        } else {
            let _ = line.push_str(row.text.as_str());
        }

        // Colour: a sent row of an encrypted chat is green when acknowledged and
        // grey while sending; everything else is cyan when encrypted, muted when public.
        let color = if row.from_me && row.encrypted && matches!(conversation, ConversationId::Direct(_))
        {
            if row.acknowledged { ACK_GREEN } else { MUTED }
        } else if row.encrypted {
            CYAN
        } else {
            MUTED
        };
        text(display, line.as_str(), 12, row_y + 2, color, false)?;

        // SENDING.. sits on the right edge until the ACK turns the row green.
        if row.from_me
            && row.encrypted
            && !row.acknowledged
            && matches!(conversation, ConversationId::Direct(_))
        {
            text(display, "SENDING..", 260, row_y + 2, WARNING, false)?;
        }
    }
    Ok(())
}

/// 4/ Repaint only the chat composer when its text, nibble, or cursor changes.
pub fn draw_input<D>(display: &mut D, input: &str, pending_hex: Option<u8>) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(5, 148), Size::new(310, 23))
        .into_styled(PrimitiveStyle::with_fill(BLUE))
        .draw(display)?;
    let visible_input = &input[input.len().saturating_sub(46)..];
    let mut line = String::<48>::new();
    let _ = line.push_str(visible_input);
    // Show a half-typed hex pair immediately, followed by the input cursor.
    if let Some(digit) = pending_hex {
        let _ = write!(line, "{digit:X}");
    }
    let _ = line.push('_');
    text(display, line.as_str(), 10, 154, WHITE, false)?;
    text(display, "...", 294, 162, CYAN, false)
}

/// 5/ Draw the context menu overlay when it opens.
pub fn draw_context_menu<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(162, 86), Size::new(152, 60))
        .into_styled(PrimitiveStyle::with_fill(super::style::WARNING))
        .draw(display)?;
    text(display, "SEND MESSAGE", 168, 92, WHITE, false)?;
    text(display, "0 SEND TEXT", 168, 110, CYAN, false)?;
    text(display, "1 SEND GPS POS", 168, 128, CYAN, false)?;
    text(display, "BACK CANCEL", 168, 146, MUTED, false)
}

/// 6/ Clear just the context menu overlay when it closes.
pub fn clear_context_menu<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(162, 86), Size::new(152, 60))
        .into_styled(PrimitiveStyle::with_fill(super::style::BLACK))
        .draw(display)
}

/// 7/ Show the available chat navigation controls without implying keyboard hardware.
fn draw_chat_controls<D>(display: &mut D, send_enabled: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let controls = if send_enabled {
        "HEX 2 DIGITS = 1 CHAR  ENTER: SEND MENU  UP/DOWN: BROWSE  BACK: CHATS"
    } else {
        "SENDING DISABLED   BACK: CHATS"
    };
    text(display, controls, 8, 218, WHITE, false)
}

/// Format signed microdegrees without allocating or depending on float formatting.
fn write_coordinate(line: &mut String<48>, coordinate: i32) {
    let sign = if coordinate < 0 { "-" } else { "" };
    let absolute = coordinate.unsigned_abs();
    let _ = write!(
        line,
        "{sign}{}.{:06}",
        absolute / 1_000_000,
        absolute % 1_000_000
    );
}
