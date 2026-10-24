use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};

use crate::ui::contact::ContactBook;

use super::style::{CYAN, MUTED, SELECTED, WHITE, text};

pub fn draw_main<D>(display: &mut D, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_rows(display, "COMMUNICATOR", &["CHATS", "SETTINGS"], selected)
}

pub fn draw_settings<D>(display: &mut D, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_rows(
        display,
        "SETTINGS",
        &["DEVICE", "CONTACTS", "FREQUENCY", "GPS"],
        selected,
    )
}

/// 1/ Render the permanent broadcast conversation alongside ordinary contacts.
pub fn draw_contacts<D>(
    display: &mut D,
    contacts: &ContactBook,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "CHATS", 12, 38, CYAN, true)?;
    for (index, contact) in contacts.entries().iter().enumerate() {
        let y = 76 + index as i32 * 34;
        draw_selection(display, index, selected, y)?;
        let name = contact.display_name();
        text(
            display,
            if index == selected { ">" } else { " " },
            16,
            y,
            CYAN,
            false,
        )?;
        text(display, name.as_str(), 32, y, WHITE, false)?;
    }
    if contacts.entries().len() == 1 {
        text(display, "ADD A CONTACT IN SETTINGS", 32, 110, MUTED, false)?;
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
        let y = 76 + index as i32 * 34;
        draw_selection(display, index, selected, y)?;
        text(
            display,
            if index == selected { ">" } else { " " },
            16,
            y,
            CYAN,
            false,
        )?;
        text(display, row, 32, y, WHITE, false)?;
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

fn draw_selection<D>(display: &mut D, index: usize, selected: usize, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    if index == selected {
        Rectangle::new(Point::new(8, y - 5), Size::new(304, 26))
            .into_styled(PrimitiveStyle::with_fill(SELECTED))
            .draw(display)?;
    }
    Ok(())
}
