use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};
use heapless::String;

use crate::ui::contact::ContactBook;
use crate::ui::state::SettingsSection;

use super::style::{ACK_GREEN, BLACK, CYAN, MUTED, SELECTED, UNKNOWN, WARNING, WHITE, text};

pub fn draw_main<D>(display: &mut D, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_rows(
        display,
        "COMMUNICATOR",
        &["CHATS", "CONTACTS", "SETTINGS"],
        selected,
    )
}

pub fn draw_settings<D>(display: &mut D, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_rows(
        display,
        "SETTINGS",
        &["DEVICE", "FREQUENCY", "GPS"],
        selected,
    )
}

/// 1/ Redraw only the two menu rows whose selection marker changed.
pub fn update_main_selection<D>(
    display: &mut D,
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    update_rows(
        display,
        &["CHATS", "CONTACTS", "SETTINGS"],
        previous,
        selected,
    )
}

/// 2/ Redraw only the settings rows whose selection marker changed.
pub fn update_settings_selection<D>(
    display: &mut D,
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    update_rows(display, &["DEVICE", "FREQUENCY", "GPS"], previous, selected)
}

/// 5/ Show saved contacts, passive nearby peers, and an active discovery scan.
/// 6/ Repaint just the old and new rows in the contact manager.
/// 3/ Draw the available entries for one selected Settings category.
pub fn draw_settings_section<D>(
    display: &mut D,
    section: SettingsSection,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, section.title(), 12, 38, CYAN, true)?;
    for index in 0..section.item_count() {
        draw_row(display, section.item_label(index), index, selected)?;
    }
    text(
        display,
        "UP/DOWN SELECT   ENTER VIEW",
        12,
        218,
        MUTED,
        false,
    )
}

/// 4/ Repaint only the old and new selected entries inside a Settings category.
pub fn update_settings_section_selection<D>(
    display: &mut D,
    section: SettingsSection,
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    if previous == selected {
        return Ok(());
    }
    if previous < section.item_count() {
        draw_row(display, section.item_label(previous), previous, selected)?;
    }
    if selected < section.item_count() {
        draw_row(display, section.item_label(selected), selected, selected)?;
    }
    Ok(())
}

/// 7/ Render the permanent broadcast conversation plus every saved contact,
/// each with its newest-message preview and sending / acknowledged badge.
pub fn draw_chat_list<D>(
    display: &mut D,
    contacts: &ContactBook,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "CHATS", 12, 38, CYAN, true)?;
    let indices = contacts.chat_indices();
    for (row, index) in indices.iter().enumerate() {
        if let Some(entry) = contacts.entry(*index) {
            draw_chat_row(display, entry, row, selected)?;
        }
    }
    // Deleting a single contact uses RIGHT; this last row clears every chat.
    draw_row(display, "DELETE ALL CHATS", indices.len(), selected)?;
    text(
        display,
        "ENTER OPEN   RIGHT MANAGE   BACK MENU",
        8,
        218,
        MUTED,
        false,
    )
}

/// 8/ Update the old and new selected chat rows without repainting the list.
pub fn update_chat_selection<D>(
    display: &mut D,
    contacts: &ContactBook,
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    if previous == selected {
        return Ok(());
    }
    let indices = contacts.chat_indices();
    for row in [previous, selected] {
        if row == indices.len() {
            draw_row(display, "DELETE ALL CHATS", row, selected)?;
        } else if let Some(index) = indices.get(row)
            && let Some(entry) = contacts.entry(*index)
        {
            draw_chat_row(display, entry, row, selected)?;
        }
    }
    Ok(())
}

fn draw_chat_row<D>(
    display: &mut D,
    entry: &crate::ui::contact::ContactEntry,
    index: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = 76 + index as i32 * 34;
    Rectangle::new(Point::new(8, y - 5), Size::new(304, 26))
        .into_styled(PrimitiveStyle::with_fill(if index == selected {
            SELECTED
        } else {
            BLACK
        }))
        .draw(display)?;
    text(
        display,
        if index == selected { ">" } else { " " },
        16,
        y,
        CYAN,
        false,
    )?;
    // Verified contacts and broadcast render white; peers that were only
    // heard on the air render amber until they are paired.
    let name_color = if entry.is_unknown() { UNKNOWN } else { WHITE };
    text(
        display,
        entry.name_or_id().as_str(),
        32,
        y,
        name_color,
        false,
    )?;
    // The newest record's time sits on the right of the name line.
    let time = time_label(entry.last_activity);
    text(display, time.as_str(), 276, y, MUTED, false)?;

    // One badge slot on the preview line: SENDING.. on an unacknowledged
    // message of ours, OK on the acknowledged one, nothing when received.
    let has_preview = !entry.preview.is_empty();
    if has_preview {
        text(display, entry.preview.as_str(), 32, y + 11, MUTED, false)?;
    }
    if entry.preview_from_me {
        return text(
            display,
            match entry.preview_acknowledged {
                true => "OK",
                false => "SENDING..",
            },
            250,
            y + 11,
            if entry.preview_acknowledged {
                ACK_GREEN
            } else {
                WARNING
            },
            false,
        );
    }
    Ok(())
}

/// `HH:MM` UTC for one stored record, or `--:--` before a GPS date is known.
fn time_label(timestamp: u32) -> String<8> {
    let mut label = String::new();
    if timestamp == 0 {
        let _ = label.push_str("--:--");
        return label;
    }
    let seconds = timestamp % 86_400;
    let _ = core::fmt::Write::write_fmt(
        &mut label,
        format_args!("{:02}:{:02}", seconds / 3_600, (seconds % 3_600) / 60),
    );
    label
}

fn update_rows<D>(
    display: &mut D,
    rows: &[&str],
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    if previous == selected {
        return Ok(());
    }
    if let Some(row) = rows.get(previous) {
        draw_row(display, row, previous, selected)?;
    }
    if let Some(row) = rows.get(selected) {
        draw_row(display, row, selected, selected)?;
    }
    Ok(())
}

fn draw_rows<D>(
    display: &mut D,
    title: &str,
    rows: &[&str],
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, title, 12, 38, CYAN, true)?;
    for (index, row) in rows.iter().enumerate() {
        draw_row(display, row, index, selected)?;
    }
    text(
        display,
        "UP/DOWN SELECT     ENTER OPEN",
        8,
        218,
        MUTED,
        false,
    )
}

pub fn draw_row<D>(
    display: &mut D,
    label: &str,
    index: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = 76 + index as i32 * 34;
    Rectangle::new(Point::new(8, y - 5), Size::new(304, 26))
        .into_styled(PrimitiveStyle::with_fill(if index == selected {
            SELECTED
        } else {
            BLACK
        }))
        .draw(display)?;
    text(
        display,
        if index == selected { ">" } else { " " },
        16,
        y,
        CYAN,
        false,
    )?;
    text(display, label, 32, y, WHITE, false)
}
