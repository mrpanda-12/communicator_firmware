use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};

use crate::ui::{contact::ContactBook, state::SettingsSection};

use super::style::{BLACK, CYAN, MUTED, SELECTED, WHITE, text};

pub fn draw_main<D>(display: &mut D, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
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
    D: DrawTarget<Color=Rgb565>,
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
    D: DrawTarget<Color=Rgb565>,
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
    D: DrawTarget<Color=Rgb565>,
{
    update_rows(display, &["DEVICE", "FREQUENCY", "GPS"], previous, selected)
}

/// 5/ Show saved contacts, passive nearby peers, and an active discovery scan.
pub fn draw_contact_manager<D>(
    display: &mut D,
    contacts: &ContactBook,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    text(display, "CONTACTS", 12, 38, CYAN, true)?;
    let direct_count = contacts.entries().len().saturating_sub(1);
    for index in 0..direct_count {
        if let Some(contact) = contacts.get(index + 1) {
            draw_contact_row(display, contact.display_name().as_str(), index, selected)?;
        }
    }
    if direct_count == 0 {
        text(display, "NO SAVED CONTACTS", 32, 122, MUTED, false)?;
    }
    draw_row(display, "VIEW NEARBY PEERS", direct_count, selected)?;
    draw_row(display, "SCAN FOR DEVICES", direct_count + 1, selected)?;
    text(display, "ENTER OPEN     BACK RETURN", 8, 218, MUTED, false)
}

/// 6/ Repaint just the old and new rows in the contact manager.
pub fn update_contact_manager_selection<D>(
    display: &mut D,
    contacts: &ContactBook,
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    if previous == selected {
        return Ok(());
    }
    let direct_count = contacts.entries().len().saturating_sub(1);
    for index in [previous, selected] {
        if index == direct_count {
            draw_row(display, "VIEW NEARBY PEERS", index, selected)?;
        } else if index == direct_count + 1 {
            draw_row(display, "SCAN FOR DEVICES", index, selected)?;
        } else if let Some(contact) = contacts.get(index + 1) {
            draw_contact_row(display, contact.display_name().as_str(), index, selected)?;
        }
    }
    Ok(())
}

/// 3/ Draw the available entries for one selected Settings category.
pub fn draw_settings_section<D>(
    display: &mut D,
    section: SettingsSection,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    text(display, section.title(), 12, 38, CYAN, true)?;
    for index in 0..section.item_count() {
        draw_row(display, section.item_label(index), index, selected)?;
    }
    text(
        display,
        "UP/DOWN SELECT     ENTER VIEW",
        8,
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
    D: DrawTarget<Color=Rgb565>,
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

/// 1/ Render the permanent broadcast conversation alongside ordinary contacts.
pub fn draw_contacts<D>(
    display: &mut D,
    contacts: &ContactBook,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    text(display, "CHATS", 12, 38, CYAN, true)?;
    for (index, contact) in contacts.entries().iter().enumerate() {
        draw_contact_row(display, contact.display_name().as_str(), index, selected)?;
    }
    if contacts.entries().len() == 1 {
        text(
            display,
            "ADD A CONTACT FROM CONTACTS",
            32,
            110,
            MUTED,
            false,
        )?;
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

/// 3/ Update the old and new selected contact rows without repainting the list.
pub fn update_contact_selection<D>(
    display: &mut D,
    contacts: &ContactBook,
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    if previous == selected {
        return Ok(());
    }
    if let Some(contact) = contacts.get(previous) {
        draw_contact_row(display, contact.display_name().as_str(), previous, selected)?;
    }
    if let Some(contact) = contacts.get(selected) {
        draw_contact_row(display, contact.display_name().as_str(), selected, selected)?;
    }
    Ok(())
}

fn update_rows<D>(
    display: &mut D,
    rows: &[&str],
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
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
    D: DrawTarget<Color=Rgb565>,
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

fn draw_row<D>(display: &mut D, label: &str, index: usize, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
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

fn draw_contact_row<D>(
    display: &mut D,
    label: &str,
    index: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
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
