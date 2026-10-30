//! One module per screen: contacts, announce, devices, add, settings.

pub mod add_contact;
pub mod announce;
pub mod contacts;
pub mod devices;
pub mod settings;

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};

use crate::network::types::DiscoveryList;
use crate::hex::hex_device_id;
use crate::ui::render::menu::draw_row;
use crate::ui::render::style::{CYAN, MUTED, WHITE, text};

/// Rows a device list shows at once; the list scrolls to the selection.
const VISIBLE_DEVICES: usize = 5;

/// One scrollable device list shared by the announce and search screens.
pub fn draw_device_list<D>(
    display: &mut D,
    title: &str,
    subtitle: &str,
    devices: &DiscoveryList,
    selected: usize,
    footer: &str,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, title, 12, 38, CYAN, true)?;
    text(display, subtitle, 12, 62, MUTED, false)?;
    if devices.is_empty() {
        text(display, "WAITING FOR DEVICES", 12, 92, WHITE, false)?;
    }
    let first = selected.saturating_sub(VISIBLE_DEVICES - 1);
    for (row, device_id) in devices.iter().skip(first).take(VISIBLE_DEVICES).enumerate() {
        let label = hex_device_id(device_id);
        draw_row(display, label.as_str(), row, selected - first)?;
    }
    text(display, footer, 8, 218, WHITE, false)
}

/// One selectable action row.
pub fn draw_option<D>(display: &mut D, label: &str, y: i32, selected: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(
        display,
        if selected { ">" } else { " " },
        12,
        y,
        CYAN,
        false,
    )?;
    text(
        display,
        label,
        30,
        y,
        if selected { WHITE } else { MUTED },
        false,
    )
}
