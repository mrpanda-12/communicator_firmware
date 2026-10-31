//! Blocked device IDs: the peers whose addressed traffic this device drops.

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};

use crate::hex::hex_device_id;
use crate::storage::block::BlockList;
use crate::ui::render::style::{CYAN, MUTED, text};

use super::menu;

/// Rows the list shows at once; it scrolls with the selection.
const VISIBLE_ROWS: usize = 4;

/// List the blocked device IDs; ENTER unblocks the picked one.
pub fn draw<D>(display: &mut D, blocked: &BlockList, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "BLOCKED DEVICES", 12, 38, CYAN, true)?;
    let first = selected.saturating_sub(VISIBLE_ROWS - 1);
    for (row, device_id) in blocked
        .ids()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
    {
        let label = hex_device_id(device_id);
        menu::draw_row(display, label.as_str(), row - first, selected - first)?;
    }
    if blocked.is_empty() {
        text(display, "NOTHING BLOCKED", 12, 92, MUTED, false)?;
    }
    text(
        display,
        "ENTER: UNBLOCK   BACK: SETTINGS",
        8,
        218,
        MUTED,
        false,
    )
}
