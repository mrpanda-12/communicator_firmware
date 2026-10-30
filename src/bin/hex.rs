//! Shared text formatting for identifiers.

use core::fmt::Write;
use heapless::String;

/// Characters in one full device ID: two uppercase hex digits per byte.
pub const DEVICE_ID_CHARS: usize = 32;

/// Write `bytes` as uppercase hex, optionally separated between bytes.
pub fn write_hex(out: &mut impl Write, bytes: &[u8], separator: Option<char>) -> core::fmt::Result {
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            if let Some(separator) = separator {
                out.write_char(separator)?;
            }
        }
        write!(out, "{byte:02X}")?;
    }
    Ok(())
}

/// Uppercase hex form of a device ID, without separators.
pub fn hex_device_id(device_id: &[u8; 16]) -> String<DEVICE_ID_CHARS> {
    let mut label = String::new();
    // A full ID is exactly the capacity, so the write cannot fail.
    let _ = write_hex(&mut label, device_id, None);
    label
}
