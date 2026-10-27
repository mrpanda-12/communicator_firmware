use core::fmt::Write as _;

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};
use heapless::String;

use crate::{
    network::discovery::DiscoveryList,
    radio::types::RadioConfig,
    ui::state::{PairingUiStatus, SettingsSection},
};

use super::{
    menu,
    style::{CYAN, MUTED, WARNING, WHITE, text},
};

/// 1/ Draw one category's setting rows or the selected row's detail page.
pub fn draw<D>(display: &mut D, section: SettingsSection, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
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
    D: DrawTarget<Color=Rgb565>,
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
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    text(display, section.title(), 12, 38, CYAN, true)?;
    text(display, section.item_label(item), 12, 78, WHITE, false)?;
    if section == SettingsSection::Device && item == 0 {
        draw_device_info(display, device_id, firmware_version)?;
    } else if section == SettingsSection::Frequency && item == 0 {
        draw_radio_info(display, radio_config)?;
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

/// 4/ Display recent sender IDs heard passively in any valid packet.
pub fn draw_device_search<D>(
    display: &mut D,
    discovered: &DiscoveryList,
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    text(display, "NEARBY PEERS", 12, 38, CYAN, true)?;
    text(
        display,
        "PASSIVE + SCAN DISCOVERY",
        12,
        68,
        MUTED,
        false,
    )?;
    if let Some(device_id) = discovered.get(selected) {
        text(display, "UP/DOWN SELECTS A DEVICE", 12, 94, MUTED, false)?;
        let identity = format_device_id(device_id);
        text(display, identity.as_str(), 12, 122, WHITE, false)?;
        text(display, "ENTER VIEW   BACK CONTACTS", 8, 218, WHITE, false)
    } else {
        text(display, "NO PEERS HEARD YET", 12, 112, WHITE, false)?;
        text(display, "BACK: CONTACTS", 8, 218, WHITE, false)
    }
}

/// 5/ Show that a recently heard peer can be messaged without adding it as a contact.
pub fn draw_discovered_device<D>(
    display: &mut D,
    device_id: &[u8; 16],
    selected: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    text(display, "NEARBY PEER", 12, 38, CYAN, true)?;
    let identity = format_device_id(device_id);
    text(display, identity.as_str(), 12, 78, WHITE, false)?;
    draw_option(display, "INTRODUCE (UNENCRYPTED)", 112, selected == 0)?;
    draw_option(display, "PAIR WITH SECRET", 142, selected == 1)?;
    text(display, "UP/DOWN SELECT  ENTER OPEN", 8, 218, WHITE, false)
}

pub fn draw_pairing_entry<D>(
    display: &mut D,
    device_id: &[u8; 16],
    secret: &str,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    text(display, "PAIR WITH SHARED SECRET", 12, 38, CYAN, true)?;
    let identity = format_device_id(device_id);
    text(display, identity.as_str(), 12, 72, WHITE, false)?;
    let mut masked = String::<64>::new();
    for _ in secret.bytes() {
        let _ = masked.push('*');
    }
    text(display, masked.as_str(), 12, 112, WHITE, false)?;
    text(display, "USE 16+ RANDOM CHARACTERS", 12, 146, MUTED, false)?;
    text(display, "LOWER ID STARTS; OTHER WAITS", 12, 168, MUTED, false)?;
    text(display, "ENTER: PAIR   BACK: CANCEL", 8, 218, WHITE, false)
}

pub fn draw_pairing_status<D>(
    display: &mut D,
    device_id: &[u8; 16],
    status: PairingUiStatus,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
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
    D: DrawTarget<Color=Rgb565>,
{
    text(
        display,
        if selected { ">" } else { " " },
        12,
        y,
        CYAN,
        false,
    )?;
    text(display, label, 30, y, if selected { WHITE } else { MUTED }, false)
}

/// 7/ Show the outcome of a user-requested broadcast announcement.
pub fn draw_announcement_status<D>(
    display: &mut D,
    status: crate::ui::state::AnnouncementStatus,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    use crate::ui::state::AnnouncementStatus;

    text(display, "DEVICE ANNOUNCEMENT", 12, 38, CYAN, true)?;
    match status {
        AnnouncementStatus::Sending => {
            text(display, "ADDING TO RADIO QUEUE", 12, 92, MUTED, false)?;
        }
        AnnouncementStatus::Queued => {
            text(display, "ANNOUNCEMENT QUEUED", 12, 92, WHITE, false)?;
            text(
                display,
                "LISTENERS CAN SEE YOUR ID",
                12,
                114,
                WARNING,
                false,
            )?;
        }
        AnnouncementStatus::Failed => {
            text(display, "RADIO QUEUE IS FULL", 12, 92, WARNING, false)?;
        }
        AnnouncementStatus::NoIdentity => {
            text(display, "DEVICE ID UNAVAILABLE", 12, 92, WARNING, false)?;
        }
    }
    text(display, "ENTER / BACK: CONTACTS", 8, 218, WHITE, false)
}

/// 6/ Require an explicit acknowledgement before opening an unencrypted DM.
pub fn draw_unencrypted_dm_warning<D>(display: &mut D, device_id: &[u8; 16]) -> Result<(), D::Error>
where
    D: DrawTarget<Color=Rgb565>,
{
    text(
        display,
        "UNENCRYPTED DM",
        12,
        38,
        super::style::WARNING,
        true,
    )?;
    let identity = format_device_id(device_id);
    text(display, identity.as_str(), 12, 78, WHITE, false)?;
    text(
        display,
        "ANYONE CAN READ THESE MESSAGES",
        12,
        108,
        super::style::WARNING,
        false,
    )?;
    text(
        display,
        "DO NOT SEND PRIVATE DETAILS",
        12,
        128,
        super::style::WARNING,
        false,
    )?;
    text(display, "ENTER: CONTINUE", 12, 172, WHITE, false)?;
    text(display, "BACK: CANCEL", 12, 218, WHITE, false)
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
    D: DrawTarget<Color=Rgb565>,
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
    D: DrawTarget<Color=Rgb565>,
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
