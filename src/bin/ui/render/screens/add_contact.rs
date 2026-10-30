//! Add-contact flow: confirm, name, and password screens.

use core::fmt::Write as _;

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};
use heapless::String;

use crate::ui::render::style::{CYAN, MUTED, WHITE, text};

use super::draw_option;
use crate::hex::hex_device_id;

/// Confirm adding a heard peer; asking to add only nudges it, no key is made.
pub fn draw_confirm<D>(
    display: &mut D,
    device_id: &[u8; 16],
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "ADD TO CONTACTS?", 12, 38, CYAN, true)?;
    let identity = hex_device_id(device_id);
    text(display, identity.as_str(), 12, 78, WHITE, false)?;
    draw_option(display, "ADD CONTACT", 112, selected == 0)?;
    draw_option(display, "ASK TO ADD", 142, selected == 1)?;
    text(display, "ASK ONLY SENDS A REQUEST", 12, 176, MUTED, false)?;
    text(display, "UP/DOWN SELECT  ENTER OK", 8, 218, WHITE, false)
}

/// One-line contact name; an empty name keeps the device ID.
pub fn draw_name<D>(
    display: &mut D,
    device_id: &[u8; 16],
    name: &str,
    pending_hex: Option<u8>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "CONTACT NAME", 12, 38, CYAN, true)?;
    let identity = hex_device_id(device_id);
    text(display, identity.as_str(), 12, 72, MUTED, false)?;
    let mut line = String::<36>::new();
    let _ = line.push_str(name);
    if let Some(digit) = pending_hex {
        let _ = write!(line, "{digit:X}");
    }
    let _ = line.push('_');
    text(display, line.as_str(), 12, 112, WHITE, false)?;
    text(display, "EMPTY NAME SHOWS DEVICE ID", 12, 146, MUTED, false)?;
    text(display, "ENTER ACCEPT   BACK CANCEL", 8, 218, WHITE, false)
}

/// Shared-secret password for the pairing handshake.
pub fn draw_secret<D>(
    display: &mut D,
    device_id: &[u8; 16],
    name: &str,
    secret: &str,
    pending_hex: Option<u8>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "CONTACT PASSWORD", 12, 38, CYAN, true)?;
    let identity = hex_device_id(device_id);
    text(display, identity.as_str(), 12, 66, MUTED, false)?;
    let label = if name.is_empty() {
        "NAME: DEVICE ID"
    } else {
        name
    };
    text(display, label, 12, 86, WHITE, false)?;
    let mut masked = String::<66>::new();
    for _ in secret.bytes() {
        let _ = masked.push('*');
    }
    // A half-typed hex pair and the input cursor stay visible while masking.
    if let Some(digit) = pending_hex {
        let _ = write!(masked, "{digit:X}");
    }
    let _ = masked.push('_');
    text(display, masked.as_str(), 12, 116, WHITE, false)?;
    text(display, "1 TO 64 CHARACTERS", 12, 150, MUTED, false)?;
    text(display, "BOTH SIDES TYPE THE SAME", 12, 172, MUTED, false)?;
    text(display, "ENTER SAVE   BACK CANCEL", 8, 218, WHITE, false)
}
