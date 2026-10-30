//! Devices screen: active announcements and passively heard peers in one list.

use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};

use crate::network::types::DiscoveryList;
use crate::ui::render::style::{CYAN, MUTED, SELECTED, WHITE, text};

use crate::hex::hex_device_id;

/// Lines drawn at once, section headers included.
const VISIBLE_LINES: usize = 7;
const TOP: i32 = 56;
const LINE_HEIGHT: i32 = 22;

/// One on-screen line: a section header or a peer.
enum Line {
    Header(&'static str),
    Device([u8; 16]),
}

/// Line holding the picked peer; active peers come before passive ones.
fn selected_line(active_len: usize, recent_len: usize, selected: usize) -> usize {
    let total = active_len + recent_len;
    if total == 0 {
        return 0;
    }
    let selected = selected.min(total - 1);
    if selected < active_len {
        selected + 1
    } else {
        active_len + 2 + (selected - active_len)
    }
}

/// Line `index` of the combined list: ACTIVE header, its peers, RECENTLY HEARD
/// header, then its peers.
fn line_at(active: &DiscoveryList, recent: &DiscoveryList, index: usize) -> Option<Line> {
    let active_len = active.len();
    if index == 0 {
        return Some(Line::Header(if active.is_empty() {
            "ACTIVE (NONE)"
        } else {
            "ACTIVE"
        }));
    }
    if index <= active_len {
        return active.get(index - 1).copied().map(Line::Device);
    }
    if index == active_len + 1 {
        return Some(Line::Header(if recent.is_empty() {
            "RECENTLY HEARD (NONE)"
        } else {
            "RECENTLY HEARD"
        }));
    }
    recent
        .get(index - active_len - 2)
        .copied()
        .map(Line::Device)
}

/// Draw both sections; the window scrolls so the picked peer stays visible.
pub fn draw<D>(
    display: &mut D,
    active: &DiscoveryList,
    recent: &DiscoveryList,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "DEVICES", 12, 38, CYAN, true)?;
    let picked = selected_line(active.len(), recent.len(), selected);
    let first = picked.saturating_sub(VISIBLE_LINES - 1);
    for offset in 0..VISIBLE_LINES {
        let index = first + offset;
        let Some(line) = line_at(active, recent, index) else {
            break;
        };
        let y = TOP + offset as i32 * LINE_HEIGHT;
        match line {
            Line::Header(label) => text(display, label, 8, y, CYAN, false)?,
            Line::Device(device_id) => {
                let chosen = index == picked;
                if chosen {
                    Rectangle::new(Point::new(8, y - 4), Size::new(304, 20))
                        .into_styled(PrimitiveStyle::with_fill(SELECTED))
                        .draw(display)?;
                }
                text(display, if chosen { ">" } else { " " }, 12, y, CYAN, false)?;
                let label = hex_device_id(&device_id);
                text(display, label.as_str(), 24, y, WHITE, false)?;
            }
        }
    }
    if active.is_empty() && recent.is_empty() {
        text(display, "NOTHING HEARD YET", 8, 110, MUTED, false)?;
    }
    text(display, "ENTER OPEN   BACK CONTACTS", 8, 218, MUTED, false)
}
