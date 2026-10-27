use core::fmt::Write as _;

use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{Line, PrimitiveStyle, Rectangle},
};

use crate::ui::{contact::ConversationMessage, state::ConversationKind};
use heapless::{String, Vec};

use super::style::{ACK_GREEN, BLUE, CYAN, MUTED, WHITE, text};

/// 1/ Draw the selected conversation, composer, and context menu.
pub fn draw<D>(
    display: &mut D,
    conversation: ConversationKind,
    messages: &[ConversationMessage],
    input: &str,
    menu_open: bool,
    unencrypted_dm: bool,
    send_enabled: bool,
    selected_message: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_header(display, conversation, unencrypted_dm)?;
    draw_message_panel(display, conversation, messages, selected_message)?;
    draw_input(display, input)?;
    draw_chat_controls(display, send_enabled)?;

    if menu_open {
        draw_context_menu(display, unencrypted_dm)
    } else {
        Ok(())
    }
}

/// 2/ Repaint only the chat title row when the active conversation changes.
pub fn draw_header<D>(
    display: &mut D,
    conversation: ConversationKind,
    unencrypted_dm: bool,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let title = match (conversation, unencrypted_dm) {
        (ConversationKind::Direct(_), true) => "DIRECT [UNENCRYPTED]",
        (ConversationKind::Direct(_), false) => "DIRECT MESSAGE",
        (ConversationKind::Broadcast, _) => "BROADCAST [PUBLIC - UNENCRYPTED]",
    };
    Rectangle::new(Point::new(5, 28), Size::new(310, 22))
        .into_styled(PrimitiveStyle::with_fill(super::style::BLACK))
        .draw(display)?;
    text(display, "<", 8, 34, CYAN, false)?;
    text(display, title, 28, 34, WHITE, false)
}

pub fn draw_message_panel<D>(
    display: &mut D,
    conversation: ConversationKind,
    messages: &[ConversationMessage],
    selected_message: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(5, 54), Size::new(310, 91))
        .into_styled(PrimitiveStyle::with_stroke(MUTED, 1))
        .draw(display)?;
    let peer_id = match conversation {
        ConversationKind::Broadcast => [0; 16],
        ConversationKind::Direct(device_id) => device_id,
    };
    let mut visible = Vec::<&ConversationMessage, 4>::new();
    for message in messages
        .iter()
        .rev()
        .filter(|message| message.peer_id == peer_id)
        .take(4)
    {
        let _ = visible.push(message);
    }
    for (index, message) in visible.iter().rev().enumerate() {
        let row_y = 59 + index as i32 * 19;
        if index == selected_message {
            Rectangle::new(Point::new(7, row_y), Size::new(306, 17))
                .into_styled(PrimitiveStyle::with_fill(super::style::SELECTED))
                .draw(display)?;
        }
        let mut line = String::<48>::new();
        let label = if message.coordinates.is_some() && message.encrypted {
            "SECURE GPS"
        } else if message.coordinates.is_some() {
            "OPEN GPS"
        } else if message.from_local {
            "ME"
        } else if message.encrypted {
            "SECURE"
        } else {
            "OPEN"
        };
        let _ = write!(line, "{label}: ");
        if let Some((latitude, longitude)) = message.coordinates {
            write_coordinate(&mut line, latitude);
            let _ = line.push(',');
            write_coordinate(&mut line, longitude);
        } else {
            for character in message.text.chars().take(28) {
                let _ = line.push(character);
            }
        }
        text(
            display,
            line.as_str(),
            12,
            row_y + 2,
            if message.encrypted { CYAN } else { MUTED },
            false,
        )?;
        if message.from_local
            && message.encrypted
            && message.acknowledged
            && matches!(conversation, ConversationKind::Direct(_))
        {
            let y = row_y + 5;
            Line::new(Point::new(295, y + 3), Point::new(298, y + 6))
                .into_styled(PrimitiveStyle::with_stroke(ACK_GREEN, 1))
                .draw(display)?;
            Line::new(Point::new(298, y + 6), Point::new(304, y))
                .into_styled(PrimitiveStyle::with_stroke(ACK_GREEN, 1))
                .draw(display)?;
        }
    }
    Ok(())
}

/// 3/ Repaint only the chat composer when its text changes.
pub fn draw_input<D>(display: &mut D, input: &str) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(5, 148), Size::new(310, 23))
        .into_styled(PrimitiveStyle::with_fill(BLUE))
        .draw(display)?;
    let visible_input = &input[input.len().saturating_sub(48)..];
    text(display, visible_input, 10, 154, WHITE, false)?;
    text(display, "...", 294, 162, CYAN, false)
}

/// 4/ Draw the context menu overlay when it opens.
pub fn draw_context_menu<D>(display: &mut D, unencrypted_dm: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(162, 86), Size::new(152, 60))
        .into_styled(PrimitiveStyle::with_fill(super::style::WARNING))
        .draw(display)?;
    text(
        display,
        if unencrypted_dm {
            "SEND UNENCRYPTED?"
        } else {
            "SEND ENCRYPTED?"
        },
        168,
        92,
        WHITE,
        false,
    )?;
    text(display, "ENTER SEND", 168, 110, WHITE, false)?;
    text(display, "BACK CANCEL", 168, 128, WHITE, false)
}

/// 5/ Clear just the context menu overlay when it closes.
pub fn clear_context_menu<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(162, 86), Size::new(152, 60))
        .into_styled(PrimitiveStyle::with_fill(super::style::BLACK))
        .draw(display)
}

/// 6/ Show the available chat navigation controls without implying keyboard hardware.
fn draw_chat_controls<D>(display: &mut D, send_enabled: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let controls = if send_enabled {
        "UP/DN SELECT  ENTER GPS/SEND  BACK"
    } else {
        "SENDING DISABLED   BACK: CHATS"
    };
    text(display, controls, 8, 218, WHITE, false)
}

/// Format signed microdegrees without allocating or depending on floating-point display support.
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
