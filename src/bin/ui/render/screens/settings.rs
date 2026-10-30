use core::fmt::Write as _;

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};
use heapless::String;

use crate::{
    radio::types::RadioConfig,
    ui::state::{
        FREQUENCY_PRESETS, FrequencyUiStatus, PairingUiStatus, ResetUiStatus, SettingsSection,
    },
};

use crate::ui::render::{
    menu,
    style::{CYAN, MUTED, WARNING, WHITE, text},
};

/// 1/ Draw one category's setting rows or the selected row's detail page.
pub fn draw<D>(display: &mut D, section: SettingsSection, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    menu::draw_settings_section(display, section, selected)
}

/// 2/ Update just the selection rows when the highlighted setting changes.
pub fn update_selection<D>(
    display: &mut D,
    section: SettingsSection,
    previous: usize,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    menu::update_settings_section_selection(display, section, previous, selected)
}

/// 3/ Show the persisted device identity and describe other setting actions.
pub fn draw_item<D>(
    display: &mut D,
    section: SettingsSection,
    item: usize,
    device_id: Option<[u8; 16]>,
    firmware_version: &str,
    radio_config: RadioConfig,
    timezone_offset_minutes: i32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, section.title(), 12, 38, CYAN, true)?;
    text(display, section.item_label(item), 12, 78, WHITE, false)?;
    if section == SettingsSection::Device && item == 0 {
        draw_device_info(display, device_id, firmware_version)?;
    } else if section == SettingsSection::Device && item == 1 {
        draw_packet_ids(display)?;
    } else if section == SettingsSection::Frequency && item == 0 {
        draw_radio_info(display, radio_config)?;
    } else if section.is_timezone_item(item) {
        draw_timezone_offset(display, timezone_offset_minutes)?;
    } else {
        text(
            display,
            section.item_description(item),
            12,
            104,
            MUTED,
            false,
        )?;
    }
    text(display, "BACK: OPTIONS", 8, 218, WHITE, false)
}

/// Show the last assigned IDs and how much of the 32-bit space is left.
fn draw_packet_ids<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let next = crate::packet::packet_data::current_packet_id();
    let last_packet = next.saturating_sub(1);
    let last_message = crate::packet::packet_data::last_message_id();
    let used = u64::from(next) * 100 / u64::from(u32::MAX);
    let left = 100u64.saturating_sub(used);

    for (label, value, y) in [
        ("LAST PACKET", last_packet, 104),
        ("LAST MESSAGE", last_message, 126),
        ("NEXT PACKET", next, 148),
    ] {
        let mut line = String::<32>::new();
        let _ = write!(line, "{label} {:08X}", value);
        text(display, line.as_str(), 12, y, WHITE, false)?;
    }

    let mut remaining = String::<32>::new();
    let _ = write!(
        remaining,
        "IDS LEFT {left}% {}",
        u32::MAX.saturating_sub(next)
    );
    text(display, remaining.as_str(), 12, 170, CYAN, false)
}

/// Show the current UTC offset and how to change it.
fn draw_timezone_offset<D>(display: &mut D, timezone_offset_minutes: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let sign = if timezone_offset_minutes < 0 {
        "-"
    } else {
        "+"
    };
    let absolute = timezone_offset_minutes.unsigned_abs();
    let mut offset = String::<16>::new();
    let _ = write!(offset, "{sign}{:02}:{:02}", absolute / 60, absolute % 60);
    text(display, offset.as_str(), 12, 104, WHITE, false)?;
    text(
        display,
        "UP/DOWN ADJUST (30 MIN STEPS)",
        12,
        134,
        MUTED,
        false,
    )?;
    text(display, "CHANGE IS SAVED AT ONCE", 12, 154, MUTED, false)
}

/// Show the name, password, block, and delete actions for one contact.
/// 4/ Display heard sender IDs, passively or while an announce scan runs.
/// 5/ Confirm adding one heard peer to the contact list.
/// One-line contact name; an empty name keeps the device ID.
/// Shared-secret password for the pairing handshake.
pub fn draw_pairing_status<D>(
    display: &mut D,
    device_id: &[u8; 16],
    status: PairingUiStatus,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "SECURE PAIRING", 12, 38, CYAN, true)?;
    let identity = format_device_id(device_id);
    text(display, identity.as_str(), 12, 78, WHITE, false)?;
    let (label, color) = match status {
        PairingUiStatus::Starting => ("STARTING PAIR REQUEST", MUTED),
        PairingUiStatus::Waiting => ("WAITING FOR LOWER ID", MUTED),
        PairingUiStatus::RequestSent => ("WAITING FOR CONFIRMATION", MUTED),
        PairingUiStatus::Paired => ("KEY VERIFIED; CONTACT SAVED", CYAN),
        PairingUiStatus::Failed => ("PAIRING FAILED; NOT SAVED", WARNING),
    };
    text(display, label, 12, 120, color, false)?;
    text(display, "ENTER / BACK: RETURN", 8, 218, WHITE, false)
}

fn draw_option<D>(display: &mut D, label: &str, y: i32, selected: bool) -> Result<(), D::Error>
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

/// 6/ Require an explicit acknowledgement before opening an unencrypted DM.
pub fn draw_unencrypted_dm_warning<D>(display: &mut D, device_id: &[u8; 16]) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(
        display,
        "UNENCRYPTED DM",
        12,
        38,
        crate::ui::render::style::WARNING,
        true,
    )?;
    let identity = format_device_id(device_id);
    text(display, identity.as_str(), 12, 78, WHITE, false)?;
    text(
        display,
        "ANYONE CAN READ THESE MESSAGES",
        12,
        108,
        crate::ui::render::style::WARNING,
        false,
    )?;
    text(
        display,
        "DO NOT SEND PRIVATE DETAILS",
        12,
        128,
        crate::ui::render::style::WARNING,
        false,
    )?;
    text(display, "ENTER: CONTINUE", 12, 172, WHITE, false)?;
    text(display, "BACK: CANCEL", 12, 218, WHITE, false)
}

/// Show the outcome of the requested identity regeneration.
pub fn draw_identity_reset<D>(
    display: &mut D,
    device_id: Option<[u8; 16]>,
    status: ResetUiStatus,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "RESET IDENTITY", 12, 38, CYAN, true)?;
    match device_id {
        Some(device_id) => {
            let identity = format_device_id(&device_id);
            text(display, identity.as_str(), 12, 72, MUTED, false)?;
        }
        None => text(display, "NO IDENTITY YET", 12, 72, MUTED, false)?,
    }
    let (label, color) = match status {
        ResetUiStatus::Working => ("REGENERATING ...", MUTED),
        ResetUiStatus::Done => ("NEW IDENTITY SAVED", CYAN),
        ResetUiStatus::Failed => ("FAILED; ID UNCHANGED", WARNING),
    };
    text(display, label, 12, 108, color, false)?;
    text(
        display,
        "PEERS MUST RE-PAIR THE NEW ID",
        12,
        132,
        WARNING,
        false,
    )?;
    text(
        display,
        "CONTACTS AND HISTORY ARE KEPT",
        12,
        152,
        MUTED,
        false,
    )?;
    text(display, "ENTER / BACK: SETTINGS", 8, 218, WHITE, false)
}

/// Show the offered frequency presets for one radio.
pub fn draw_frequency_change<D>(display: &mut D, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "CHANGE FREQUENCY", 12, 38, CYAN, true)?;
    for (index, (_, label)) in FREQUENCY_PRESETS.iter().enumerate() {
        draw_option(display, label, 96 + index as i32 * 30, selected == index)?;
    }
    text(
        display,
        "EVERY PEER MUST RETUNE TOO",
        12,
        170,
        WARNING,
        false,
    )?;
    text(
        display,
        "UP/DOWN SELECT  ENTER: RETUNE",
        8,
        218,
        WHITE,
        false,
    )
}

/// Show the outcome of one retune request.
pub fn draw_frequency_status<D>(display: &mut D, status: FrequencyUiStatus) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(display, "CHANGE FREQUENCY", 12, 38, CYAN, true)?;
    match status {
        FrequencyUiStatus::Queued => {
            text(display, "RETUNE QUEUED", 12, 92, CYAN, false)?;
            text(
                display,
                "APPLIED AT THE NEXT IDLE BOUNDARY",
                12,
                114,
                MUTED,
                false,
            )?;
        }
        FrequencyUiStatus::Busy => {
            text(display, "RADIO BUSY; TRY AGAIN", 12, 92, WARNING, false)?;
        }
    }
    text(display, "ENTER / BACK: SETTINGS", 8, 218, WHITE, false)
}

fn format_device_id(device_id: &[u8; 16]) -> String<40> {
    let mut identity = String::<40>::new();
    for byte in device_id {
        let _ = write!(identity, "{byte:02X}");
    }
    identity
}

/// 8/ Show the configured frequency and transmit power without duplicating radio values.
fn draw_radio_info<D>(display: &mut D, config: RadioConfig) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let whole_mhz = config.frequency_hz / 1_000_000;
    let fraction_khz = (config.frequency_hz % 1_000_000) / 1_000;
    let mut frequency = String::<32>::new();
    let _ = write!(frequency, "FREQUENCY: {whole_mhz}.{fraction_khz:03} MHz");
    text(display, frequency.as_str(), 12, 104, WHITE, false)?;

    let mut power = String::<24>::new();
    let _ = write!(power, "TX POWER: {} dBm", config.tx_power_dbm);
    text(display, power.as_str(), 12, 128, WHITE, false)
}

/// 9/ Show the complete device identity on one line and the firmware version below it.
fn draw_device_info<D>(
    display: &mut D,
    device_id: Option<[u8; 16]>,
    firmware_version: &str,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    if let Some(device_id) = device_id {
        let mut identity = String::<56>::new();
        let _ = identity.write_str("ID: ");
        for (index, byte) in device_id.iter().enumerate() {
            if index > 0 {
                let _ = identity.write_char(':');
            }
            let _ = write!(identity, "{byte:02X}");
        }
        text(display, identity.as_str(), 12, 104, WHITE, false)?;
    } else {
        text(display, "DEVICE ID UNAVAILABLE", 12, 104, MUTED, false)?;
    }

    let mut version = String::<32>::new();
    let _ = write!(version, "FIRMWARE: {firmware_version}");
    text(display, version.as_str(), 12, 128, WHITE, false)
}
