//! UI state: what each screen shows and the commands the run loop drains.
//!
//! Split by concern so no single file owns the whole state machine: screen
//! the render layer under `render/screens`, key handling in [`input`] and
//! [`keypad`], chat rows
//! and paging in [`chat`], outgoing messages in [`outgoing`], and the pending
//! commands plus status reports in [`commands`].

mod chat;
mod commands;
mod input;
mod keypad;
pub mod navigation;
mod outgoing;
pub mod settings;

pub use crate::ui::render::screens::{
    AnnouncementStatus, DeviceOrigin, FREQUENCY_PRESETS, FrequencyUiStatus, Key, ManageAction,
    ManageCommand, PairingCommand, PairingUiStatus, RenameCommand, ResetUiStatus, Screen,
    SettingsSection, device_list,
};
pub use chat::{ChatRow, HistoryPageRequest};
pub use outgoing::{OutgoingBody, OutgoingMessage};

use heapless::{String, Vec};

use crate::network::types::DiscoveryList;
use crate::packet::packet_data::MAX_LOGICAL_MESSAGE_SIZE;
use crate::radio::types::RadioConfig;

use crate::ui::contact::ContactBook;
use settings::Settings;

/// Chat rows the message panel draws at once.
pub const MAX_CHAT_MESSAGES: usize = 4;

/// Characters of one message shown in a chat panel row.
pub const ROW_TEXT_CHARS: usize = 28;

#[derive(Clone, Debug, PartialEq)]
pub struct UiState {
    pub screen: Screen,
    pub contacts: ContactBook,
    pub device_id: Option<[u8; 16]>,
    pub firmware_version: &'static str,
    pub radio_config: RadioConfig,
    pub discovered_devices: DiscoveryList,
    /// Peers whose own announcement was heard, for the search screen.
    pub announced_devices: DiscoveryList,
    /// Peers heard in any other packet, for the passive list.
    pub passive_devices: DiscoveryList,
    /// Chat rows of the open conversation: flash-backed, refreshed on change.
    pub messages: Vec<ChatRow, MAX_CHAT_MESSAGES>,
    /// Composer text of the open chat. It lives here instead of inside
    /// `Screen::ChatView` so the screen enum stays small.
    pub chat_input: String<MAX_LOGICAL_MESSAGE_SIZE>,
    /// Latest received-message toast, drawn over any screen.
    pub popup: Option<crate::notify::Popup>,
    /// One toast waiting for the visible one to be dismissed. A retune request
    /// outranks a plain message toast, so it is never the one dropped.
    deferred_popup: Option<crate::notify::Popup>,
    pub gps_snapshot: crate::ui::GpsSnapshot,
    pub movement_course_degrees: Option<u16>,
    pub settings: Settings,
    /// Keypad legend overlay; the full ASCII table is a later addition.
    pub help_open: bool,
    course_anchor: Option<(i32, i32)>,
    pending_message: Option<OutgoingMessage>,
    pending_manage: Option<ManageCommand>,
    pending_delete_all: bool,
    pending_timezone: Option<i32>,
    pending_announcement: bool,
    /// Stop the repeating announce scan when the user leaves the list.
    cancel_scan: bool,
    /// Peer to send a plaintext add-contact request to.
    pending_contact_request: Option<[u8; 16]>,
    pending_pairing: Option<PairingCommand>,
    /// Contact name submitted but not yet written to flash.
    pending_rename: Option<RenameCommand>,
    /// Name to give the contact a running pairing will save.
    pending_contact_name: String<32>,
    /// Regenerate the flash identity when the run loop reaches a storage slot.
    pending_reset_identity: bool,
    /// Frequency the user picked for the radio worker to retune.
    pending_frequency: Option<u32>,
    /// Frequency and contact mask of the retune requests still to send.
    pending_frequency_requests: Option<(u32, u32)>,
    pending_history_page: Option<HistoryPageRequest>,
}

impl UiState {
    /// Start at the application menu with chats selected and the radio
    /// clock showing UTC plus the saved offset.
    pub fn new(
        device_id: Option<[u8; 16]>,
        contacts: ContactBook,
        firmware_version: &'static str,
        radio_config: RadioConfig,
        discovered_devices: DiscoveryList,
        settings: Settings,
    ) -> Self {
        Self {
            screen: Screen::MainMenu { selected: 0 },
            contacts,
            device_id,
            firmware_version,
            radio_config,
            discovered_devices,
            announced_devices: DiscoveryList::new(),
            passive_devices: DiscoveryList::new(),
            settings,
            messages: Vec::new(),
            chat_input: String::new(),
            popup: None,
            deferred_popup: None,
            gps_snapshot: crate::ui::GpsSnapshot::default(),
            movement_course_degrees: None,
            help_open: false,
            course_anchor: None,
            pending_message: None,
            pending_manage: None,
            pending_delete_all: false,
            pending_timezone: None,
            pending_announcement: false,
            cancel_scan: false,
            pending_contact_request: None,
            pending_pairing: None,
            pending_rename: None,
            pending_contact_name: String::new(),
            pending_reset_identity: false,
            pending_frequency: None,
            pending_frequency_requests: None,
            pending_history_page: None,
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new(
            None,
            ContactBook::new(),
            "unknown",
            RadioConfig::default(),
            DiscoveryList::new(),
            Settings::default(),
        )
    }
}
