//! Contacts list and the per-contact actions screen.

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};
use heapless::String;

use crate::ui::contact::ContactBook;
use crate::ui::render::menu::draw_row;
use crate::ui::render::style::{CYAN, MUTED, WHITE, text};

use super::{draw_option, format_device_id};

/// Rows the contacts list shows at once; it scrolls with the selection.
const VISIBLE_ROWS: usize = 4;

/// Label of one contacts-list row: a contact, or one of the two actions.
fn row_label(contacts: &ContactBook, row: usize) -> Option<String<32>> {
    let indices = contacts.visible_indices(true);
    if row < indices.len() {
        return indices
            .get(row)
            .and_then(|index| contacts.entry(*index))
            .map(|entry| entry.display_name());
    }
    let mut label = String::new();
    match row - indices.len() {
        0 => {
            let _ = label.push_str("ADD DEVICE");
        }
        1 => {
            let _ = label.push_str("DEVICES");
        }
        _ => return None,
    }
    Some(label)
}

/// Contacts list: saved contacts, then add and the shared devices list.
pub fn draw<D>(display: &mut D, contacts: &ContactBook, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "CONTACTS", 12, 38, CYAN, true)?;
    let first = selected.saturating_sub(VISIBLE_ROWS - 1);
    for (offset, row) in (first..first + VISIBLE_ROWS).enumerate() {
        if let Some(label) = row_label(contacts, row) {
            draw_row(display, label.as_str(), offset, selected - first)?;
        }
    }
    if contacts.visible_indices(true).is_empty() {
        text(display, "ADD DEVICE TO FIND A PEER", 12, 192, MUTED, false)?;
    }
    text(display, "ENTER OPEN    RIGHT MANAGE", 8, 218, MUTED, false)
}

/// Repaint the contacts list for the new selection; scrolling shifts rows.
pub fn update_selection<D>(
    display: &mut D,
    contacts: &ContactBook,
    _previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw(display, contacts, selected)
}

/// Name, password, block, and delete actions for one contact.
pub fn draw_manage<D>(
    display: &mut D,
    device_id: &[u8; 16],
    selected: usize,
    blocked: bool,
    has_key: bool,
    name: &str,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "MANAGE CONTACT", 12, 38, CYAN, true)?;
    let identity = format_device_id(device_id);
    text(display, identity.as_str(), 12, 70, WHITE, false)?;
    draw_option(
        display,
        if name.is_empty() {
            "ADD NAME"
        } else {
            "CHANGE NAME"
        },
        104,
        selected == 0,
    )?;
    draw_option(display, "REMOVE NAME", 130, selected == 1)?;
    draw_option(
        display,
        if has_key {
            "CHANGE PASSWORD"
        } else {
            "ADD PASSWORD"
        },
        156,
        selected == 2,
    )?;
    draw_option(
        display,
        if blocked {
            "UNBLOCK CONTACT"
        } else {
            "BLOCK CONTACT"
        },
        182,
        selected == 3,
    )?;
    draw_option(display, "DELETE CHAT + CONTACT", 208, selected == 4)?;
    text(display, "BACK: BACK TO LIST", 8, 230, MUTED, false)
}
