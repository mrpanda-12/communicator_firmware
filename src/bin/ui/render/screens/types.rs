//! Screen identifiers, their on-device labels, and the values screens submit.

use heapless::String;

use crate::crypto::KEY_SIZE;
use crate::storage::message::ConversationId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    Device,
    Frequency,
    Gps,
}

impl SettingsSection {
    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Device,
            1 => Self::Frequency,
            _ => Self::Gps,
        }
    }

    pub const fn root_index(self) -> usize {
        match self {
            Self::Device => 0,
            Self::Frequency => 1,
            _ => 2,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Device => "DEVICE SETTINGS",
            Self::Frequency => "FREQUENCY SETTINGS",
            Self::Gps => "GPS SETTINGS",
        }
    }

    pub const fn item_count(self) -> usize {
        match self {
            Self::Device => 3,
            Self::Frequency => 2,
            Self::Gps => 4,
        }
    }

    pub const fn item_label(self, index: usize) -> &'static str {
        match (self, index) {
            (Self::Device, 0) => "DEVICE INFO",
            (Self::Device, 1) => "PACKET IDS",
            (Self::Device, _) => "RESET IDENTITY",
            (Self::Frequency, 0) => "CURRENT FREQUENCY",
            (Self::Frequency, _) => "CHANGE FREQUENCY",
            (Self::Gps, 0) => "FIX STATUS",
            (Self::Gps, 1) => "COORDINATES",
            (Self::Gps, 2) => "UTC TIME",
            (Self::Gps, _) => "UTC OFFSET",
        }
    }

    pub const fn item_description(self, index: usize) -> &'static str {
        match (self, index) {
            (Self::Device, 0) => "Device identity and firmware version.",
            (Self::Device, 1) => "Last packet and message ID, and space left.",
            (Self::Device, _) => "ENTER regenerates the identity in flash.",
            (Self::Frequency, 0) => "Shows configured frequency and transmit power.",
            (Self::Frequency, _) => "Pick a preset; every peer must retune too.",
            (Self::Gps, 0) => "Current fix status is also shown in the top bar.",
            (Self::Gps, 1) => "Coordinates appear here when the receiver has a fix.",
            (Self::Gps, 2) => "The top bar shows GPS time in UTC.",
            (Self::Gps, _) => "Adjust the top-bar local clock next to UTC.",
        }
    }

    /// Index of the UTC offset item inside the GPS section.
    pub fn is_timezone_item(self, item: usize) -> bool {
        matches!(self, Self::Gps) && item == 3
    }
}

/// One action the contact-manage screen can submit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManageAction {
    Delete,
    Block(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    MainMenu {
        selected: usize,
    },
    ChatsList {
        selected: usize,
    },
    ContactsList {
        selected: usize,
    },
    /// Delete / block actions for one contact, reachable from both lists.
    ContactManage {
        device_id: [u8; 16],
        selected: usize,
        from_chats: bool,
    },
    /// Active scan: announces every two seconds and lists the replies.
    AnnounceDevices {
        selected: usize,
    },
    /// One list of peers heard on the air: active, then passively heard.
    Devices {
        selected: usize,
    },
    DiscoveredDevice {
        device_id: [u8; 16],
        selected: usize,
        origin: DeviceOrigin,
    },
    UnencryptedDmWarning {
        device_id: [u8; 16],
        from_discovery: bool,
    },
    /// One-line contact name, typed as hex pairs on the keypad.
    NameEntry {
        device_id: [u8; 16],
        name: String<32>,
        cursor: usize,
        /// High nibble of a keystroke waiting for its second hex digit.
        pending_hex: Option<u8>,
        /// The add flow continues to the password step; otherwise it saves.
        add_flow: bool,
        from_chats: bool,
        origin: DeviceOrigin,
    },
    /// Shared-secret password for the pairing handshake.
    SecretEntry {
        device_id: [u8; 16],
        name: String<32>,
        secret: String<64>,
        cursor: usize,
        /// High nibble of a keystroke waiting for its second hex digit.
        pending_hex: Option<u8>,
        /// Return to the contact actions instead of the discovery list.
        from_manage: bool,
        /// Device list this password step belongs to.
        origin: DeviceOrigin,
    },
    PairingStatus {
        device_id: [u8; 16],
        status: PairingUiStatus,
    },
    ChatView {
        conversation: ConversationId,
        unencrypted_dm: bool,
        from_discovery: bool,
        cursor: usize,
        /// High nibble of a keystroke waiting for its second hex digit.
        pending_hex: Option<u8>,
        menu_open: bool,
        /// 0 sends the typed text, 1 sends the current GPS position.
        menu_index: usize,
        /// Chat row picked for coordinate navigation; `None` while typing or browsing normally.
        selected_message: Option<usize>,
        /// Oldest record still retained for this conversation.
        history_base: u32,
        /// Absolute record index of the first row in `messages`.
        history_start: u32,
        /// Next record ordinal; the exclusive upper bound of the history.
        history_total: u32,
    },
    Navigation {
        conversation: ConversationId,
        unencrypted_dm: bool,
        from_discovery: bool,
        target_latitude_microdegrees: i32,
        target_longitude_microdegrees: i32,
    },
    Settings {
        selected: usize,
    },
    SettingsSection {
        section: SettingsSection,
        selected: usize,
    },
    SettingsItem {
        section: SettingsSection,
        item: usize,
    },
    /// Identity regeneration requested; shows progress and the result.
    IdentityReset {
        status: ResetUiStatus,
    },
    /// Pick a preset frequency for the radio worker to retune.
    FrequencyChange {
        selected: usize,
    },
    /// Pick which contacts to ask before retuning. `chosen` is a bitmask over
    /// `ContactBook::visible_indices(true)`; the last row asks every tick.
    FrequencyPeers {
        frequency_hz: u32,
        cursor: usize,
        chosen: u32,
    },
    /// Outcome of one retune request.
    FrequencyStatus {
        status: FrequencyUiStatus,
    },
}

/// Which device list opened a peer's actions; BACK returns there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceOrigin {
    Announce,
    Search,
    Recent,
}

/// The list screen one origin returns to, so BACK always has a target.
pub fn device_list(origin: DeviceOrigin) -> Screen {
    match origin {
        DeviceOrigin::Announce => Screen::AnnounceDevices { selected: 0 },
        DeviceOrigin::Search | DeviceOrigin::Recent => Screen::Devices { selected: 0 },
    }
}

/// Frequencies the Change frequency screen offers, in RF order.
pub const FREQUENCY_PRESETS: [(u32, &str); 2] = [
    (869_400_000, "869.400 MHZ  SUB-GHZ"),
    (2_400_000_000, "2400.000 MHZ  2.4 GHZ"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetUiStatus {
    Working,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrequencyUiStatus {
    Queued,
    Busy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnouncementStatus {
    Queued,
    Failed,
    NoIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingUiStatus {
    Starting,
    Waiting,
    RequestSent,
    Paired,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairingCommand {
    pub peer_id: [u8; 16],
    pub key: [u8; KEY_SIZE],
}

/// A contact name the run loop still has to write to flash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameCommand {
    pub device_id: [u8; 16],
    pub name: String<32>,
}

/// Command submitted by the contact-manage screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManageCommand {
    pub device_id: [u8; 16],
    pub action: ManageAction,
    pub from_chats: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Back,
    Character(char),
    Backspace,
}
