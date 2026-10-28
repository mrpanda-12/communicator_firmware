use heapless::{String, Vec};

use crate::{
    crypto::{self, KEY_SIZE},
    hardware::keypad::KeypadKey,
    network::types::{DiscoveryList, PairingEvent},
    radio::types::RadioConfig,
    storage::contact::Contact,
    storage::message::{ConversationId, MessageKind},
};

use super::contact::{ContactBook, PREVIEW_CHARS};

/// Chat rows the message panel draws at once.
pub const MAX_CHAT_MESSAGES: usize = 4;
/// Characters of one message shown in a chat panel row.
pub const ROW_TEXT_CHARS: usize = 28;
/// Text bytes one message can carry, matching the protocol limit.
pub const MAX_TEXT: usize = crate::packet::packet_data::MAX_LOGICAL_MESSAGE_SIZE;

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
            Self::Device => 2,
            Self::Frequency => 2,
            Self::Gps => 4,
        }
    }

    pub const fn item_label(self, index: usize) -> &'static str {
        match (self, index) {
            (Self::Device, 0) => "DEVICE INFO",
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
            (Self::Device, _) => "Identity reset is not implemented in this preview.",
            (Self::Frequency, 0) => "Shows configured frequency and transmit power.",
            (Self::Frequency, _) => "Retune this radio; peers must change too.",
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
    DeviceSearch {
        selected: usize,
    },
    DiscoveredDevice {
        device_id: [u8; 16],
        selected: usize,
    },
    UnencryptedDmWarning {
        device_id: [u8; 16],
        from_discovery: bool,
    },
    PairingEntry {
        device_id: [u8; 16],
        secret: String<64>,
        cursor: usize,
        /// High nibble of a keystroke waiting for its second hex digit.
        pending_hex: Option<u8>,
    },
    PairingStatus {
        device_id: [u8; 16],
        status: PairingUiStatus,
    },
    AnnouncementStatus {
        status: AnnouncementStatus,
    },
    ChatView {
        conversation: ConversationId,
        unencrypted_dm: bool,
        from_discovery: bool,
        input: String<MAX_TEXT>,
        cursor: usize,
        /// High nibble of a keystroke waiting for its second hex digit.
        pending_hex: Option<u8>,
        menu_open: bool,
        /// 0 sends the typed text, 1 sends the current GPS position.
        menu_index: usize,
        /// Chat row picked for coordinate navigation; `None` while typing or browsing normally.
        selected_message: Option<usize>,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnouncementStatus {
    Sending,
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

/// One message the user wants to send from the composer.
#[derive(Clone, Debug, PartialEq)]
pub enum OutgoingBody {
    /// Already-typed composer text.
    Text(String<MAX_TEXT>),
    /// Coordinates from the latest GPS fix.
    Coords {
        latitude_microdegrees: i32,
        longitude_microdegrees: i32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutgoingMessage {
    pub conversation: ConversationId,
    pub unencrypted: bool,
    pub body: OutgoingBody,
}

impl OutgoingMessage {
    pub fn message_kind(&self) -> MessageKind {
        match &self.body {
            OutgoingBody::Text(_) => MessageKind::Text,
            OutgoingBody::Coords {
                latitude_microdegrees,
                longitude_microdegrees,
            } => MessageKind::Coords {
                latitude_microdegrees: *latitude_microdegrees,
                longitude_microdegrees: *longitude_microdegrees,
            },
        }
    }

    /// Optional coordinate payload when this message carries a position.
    pub fn coordinates(&self) -> Option<(i32, i32)> {
        match &self.body {
            OutgoingBody::Text(_) => None,
            OutgoingBody::Coords {
                latitude_microdegrees,
                longitude_microdegrees,
            } => Some((*latitude_microdegrees, *longitude_microdegrees)),
        }
    }

    /// Composer text, or empty for a coordinate message.
    pub fn text(&self) -> &str {
        match &self.body {
            OutgoingBody::Text(text) => text.as_str(),
            OutgoingBody::Coords { .. } => "",
        }
    }

    /// Sender-side packet payload without any packet framing.
    pub fn payload(&self) -> Vec<u8, MAX_TEXT> {
        let mut payload = Vec::new();
        match &self.body {
            OutgoingBody::Text(text) => {
                let _ = payload.extend_from_slice(text.as_bytes());
            }
            OutgoingBody::Coords {
                latitude_microdegrees,
                longitude_microdegrees,
            } => {
                let _ = payload.extend_from_slice(&latitude_microdegrees.to_be_bytes());
                let _ = payload.extend_from_slice(&longitude_microdegrees.to_be_bytes());
            }
        }
        payload
    }
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

#[derive(Clone, Debug, PartialEq)]
pub struct UiState {
    pub screen: Screen,
    pub contacts: ContactBook,
    pub device_id: Option<[u8; 16]>,
    pub firmware_version: &'static str,
    pub radio_config: RadioConfig,
    pub discovered_devices: DiscoveryList,
    /// Chat rows of the open conversation: flash-backed, refreshed on change.
    pub messages: Vec<ChatRow, MAX_CHAT_MESSAGES>,
    /// Latest received-message toast, drawn over any screen.
    pub popup: Option<crate::notify::Popup>,
    pub gps_snapshot: crate::ui::GpsSnapshot,
    pub movement_course_degrees: Option<u16>,
    pub timezone_offset_minutes: i32,
    /// Keypad legend overlay; the full ASCII table is a later addition.
    pub help_open: bool,
    course_anchor: Option<(i32, i32)>,
    pending_message: Option<OutgoingMessage>,
    pending_manage: Option<ManageCommand>,
    pending_delete_all: bool,
    pending_timezone: Option<i32>,
    pending_announcement: bool,
    pending_pairing: Option<PairingCommand>,
}

/// The single typed-entry construction site for a fresh chat composer.
/// The screen a chat entry sets: the warning when a direct chat has no key,
/// the chat view itself otherwise.
fn open_chat_target(device_has_key: bool, kind: ConversationId, from_discovery: bool) -> Screen {
    if let ConversationId::Direct(device_id) = kind {
        if !device_has_key {
            return Screen::UnencryptedDmWarning {
                device_id,
                from_discovery,
            };
        }
    }
    chat_screen(kind, false, from_discovery)
}

fn chat_screen(
    conversation: ConversationId,
    unencrypted_dm: bool,
    from_discovery: bool,
) -> Screen {
    Screen::ChatView {
        conversation,
        unencrypted_dm,
        from_discovery,
        input: String::new(),
        cursor: 0,
        pending_hex: None,
        menu_open: false,
        menu_index: 0,
        selected_message: None,
    }
}

impl UiState {
    /// 1/ Start at the application menu with chats selected and the radio
    /// clock showing UTC plus the saved offset.
    pub fn new(
        device_id: Option<[u8; 16]>,
        contacts: ContactBook,
        firmware_version: &'static str,
        radio_config: RadioConfig,
        discovered_devices: DiscoveryList,
        timezone_offset_minutes: i32,
    ) -> Self {
        Self {
            screen: Screen::MainMenu { selected: 0 },
            contacts,
            device_id,
            firmware_version,
            radio_config,
            discovered_devices,
            timezone_offset_minutes,
            messages: Vec::new(),
            popup: None,
            gps_snapshot: crate::ui::GpsSnapshot::default(),
            movement_course_degrees: None,
            help_open: false,
            course_anchor: None,
            pending_message: None,
            pending_manage: None,
            pending_delete_all: false,
            pending_timezone: None,
            pending_announcement: false,
            pending_pairing: None,
        }
    }

    /// 2/ Apply a logical key event; the input hardware can map buttons to this API.
    pub fn handle_key(&mut self, key: Key) {
        // One key always hides the toast before it acts on the screen.
        if self.popup.is_some() {
            self.popup = None;
            return;
        }
        // The help legend owns input until it is dismissed.
        if self.help_open {
            if key == Key::Back || key == Key::Enter {
                self.help_open = false;
            }
            return;
        }
        if key == Key::Enter && self.open_selected_coordinate() {
            return;
        }
        let encrypted_direct = self.active_chat_is_encrypted();
        let can_send = match &self.screen {
            Screen::ChatView {
                conversation: ConversationId::Broadcast,
                ..
            } => true,
            Screen::ChatView {
                unencrypted_dm, ..
            } => *unencrypted_dm || encrypted_direct,
            _ => false,
        };
        // These lookups run before the mutable screen match, because helper
        // methods inside its arms would borrow the whole state.
        let (chats_clear_row, chats_row_kind) = match &self.screen {
            Screen::ChatsList { selected } => (self.chat_rows(), self.chat_row_kind(*selected)),
            _ => (0, None),
        };
        let contacts_open_kind: Option<ConversationId> = match &self.screen {
            Screen::ContactsList { selected } => {
                let indices = self.contacts.visible_indices(true);
                indices
                    .get(*selected)
                    .and_then(|index| self.contacts.entry(*index))
                    .map(|entry| entry.kind)
            }
            _ => None,
        };
        match &mut self.screen {
            Screen::MainMenu { selected } => match key {
                Key::Up => *selected = (*selected + 2) % 3,
                Key::Down => *selected = (*selected + 1) % 3,
                Key::Enter => match *selected {
                    0 => self.screen = Screen::ChatsList { selected: 0 },
                    1 => self.screen = Screen::ContactsList { selected: 0 },
                    _ => self.screen = Screen::Settings { selected: 0 },
                },
                _ => {}
            },
            Screen::ChatsList { selected } => {
                // Rows: every visible contact, then a "clear everything" row.
                let is_clear = *selected + 1 == chats_clear_row;
                let row_kind = chats_row_kind;
                match key {
                    Key::Back => self.screen = Screen::MainMenu { selected: 0 },
                    Key::Up => *selected = selected.saturating_sub(1),
                    Key::Down if *selected < chats_clear_row.saturating_sub(1) => *selected += 1,
                    Key::Enter => {
                        if is_clear {
                            self.pending_delete_all = true;
                        } else if let Some(kind) = row_kind {
                            let has_key = match kind {
                                ConversationId::Direct(device_id) => self
                                    .contacts
                                    .direct_key(&device_id)
                                    .is_some(),
                                ConversationId::Broadcast => true,
                            };
                            self.screen = open_chat_target(has_key, kind, false);
                        }
                    }
                    // RIGHT opens the delete / block actions of that contact.
                    Key::Right => {
                        if let Some(ConversationId::Direct(device_id)) = row_kind {
                            self.screen = Screen::ContactManage {
                                device_id,
                                selected: 0,
                                from_chats: true,
                            };
                        }
                    }
                    _ => {}
                }
            }
            Screen::ContactsList { selected } => {
                let indices = self.contacts.visible_indices(true);
                let nearby_index = indices.len();
                let scan_index = nearby_index + 1;
                let item_count = indices.len() + 2;
                match key {
                    Key::Back => self.screen = Screen::MainMenu { selected: 1 },
                    Key::Up => *selected = (*selected + item_count - 1) % item_count,
                    Key::Down => *selected = (*selected + 1) % item_count,
                    Key::Right => {
                        if let Some(device_id) = indices
                            .get(*selected)
                            .and_then(|index| self.contacts.entry(*index))
                            .and_then(|entry| match entry.kind {
                                ConversationId::Direct(device_id) => Some(device_id),
                                ConversationId::Broadcast => None,
                            })
                        {
                            self.screen = Screen::ContactManage {
                                device_id,
                                selected: 0,
                                from_chats: false,
                            };
                        }
                    }
                    Key::Enter if *selected == nearby_index => {
                        self.screen = Screen::DeviceSearch { selected: 0 }
                    }
                    Key::Enter if *selected == scan_index => {
                        self.pending_announcement = true;
                        self.screen = Screen::DeviceSearch { selected: 0 };
                    }
                    Key::Enter => {
                        if let Some(kind) = contacts_open_kind {
                            let has_key = match kind {
                                ConversationId::Direct(device_id) => self
                                    .contacts
                                    .direct_key(&device_id)
                                    .is_some(),
                                ConversationId::Broadcast => true,
                            };
                            self.screen = open_chat_target(has_key, kind, false);
                        }
                    }
                    _ => {}
                }
            }
            Screen::ContactManage {
                device_id,
                selected,
                from_chats,
            } => {
                let have_entry = self
                    .contacts
                    .entry_by_kind(ConversationId::Direct(*device_id))
                    .is_some();
                let currently_blocked = self
                    .contacts
                    .entry_by_kind(ConversationId::Direct(*device_id))
                    .is_some_and(|entry| entry.blocked);
                let action_count = 2;
                match key {
                    Key::Back => {
                        self.screen = if *from_chats {
                            Screen::ChatsList { selected: 0 }
                        } else {
                            Screen::ContactsList { selected: 0 }
                        };
                    }
                    Key::Up => *selected = selected.saturating_sub(1),
                    Key::Down => *selected = (*selected + 1) % action_count,
                    Key::Enter if have_entry => {
                        let action = match *selected {
                            0 => ManageAction::Delete,
                            _ => ManageAction::Block(!currently_blocked),
                        };
                        self.pending_manage = Some(ManageCommand {
                            device_id: *device_id,
                            action,
                            from_chats: *from_chats,
                        });
                    }
                    _ => {}
                }
            }
            Screen::DeviceSearch { selected } => {
                let item_count = self.discovered_devices.len().max(1);
                match key {
                    Key::Back => self.screen = Screen::ContactsList { selected: 0 },
                    Key::Up => *selected = (*selected + item_count - 1) % item_count,
                    Key::Down => *selected = (*selected + 1) % item_count,
                    Key::Enter => {
                        if let Some(device_id) = self.discovered_devices.get(*selected) {
                            self.screen = Screen::DiscoveredDevice {
                                device_id: *device_id,
                                selected: 0,
                            };
                        }
                    }
                    _ => {}
                }
            }
            Screen::DiscoveredDevice {
                device_id,
                selected,
            } => match key {
                Key::Back => self.screen = Screen::DeviceSearch { selected: 0 },
                Key::Up | Key::Down => *selected = (*selected + 1) % 2,
                Key::Enter if *selected == 0 => {
                    self.screen = Screen::UnencryptedDmWarning {
                        device_id: *device_id,
                        from_discovery: true,
                    }
                }
                Key::Enter => {
                    self.screen = Screen::PairingEntry {
                        device_id: *device_id,
                        secret: String::new(),
                        cursor: 0,
                        pending_hex: None,
                    }
                }
                _ => {}
            },
            Screen::UnencryptedDmWarning {
                device_id,
                from_discovery,
            } => match key {
                Key::Back if *from_discovery => {
                    self.screen = Screen::DiscoveredDevice {
                        device_id: *device_id,
                        selected: 0,
                    }
                }
                Key::Back => self.screen = Screen::ChatsList { selected: 0 },
                Key::Enter => {
                    self.screen = chat_screen(
                        ConversationId::Direct(*device_id),
                        true,
                        *from_discovery,
                    );
                }
                _ => {}
            },
            Screen::PairingEntry {
                device_id,
                secret,
                cursor,
                ..
            } => match key {
                Key::Back => {
                    self.screen = Screen::DiscoveredDevice {
                        device_id: *device_id,
                        selected: 1,
                    }
                }
                Key::Left => *cursor = cursor.saturating_sub(1),
                Key::Right => *cursor = (*cursor + 1).min(secret.len()),
                Key::Backspace if *cursor > 0 => {
                    secret.remove(*cursor - 1);
                    *cursor -= 1;
                }
                Key::Character(ch) if ch.is_ascii() && !ch.is_control() => {
                    if secret.push(ch).is_ok() {
                        *cursor = secret.len();
                    }
                }
                Key::Enter if secret.len() >= 16 => {
                    let status = match self.device_id {
                        Some(my_id) => {
                            let derived = crypto::derive_pair_key(secret.as_bytes(), &my_id, device_id);
                            self.pending_pairing = Some(PairingCommand {
                                peer_id: *device_id,
                                key: derived,
                            });
                            if my_id < *device_id {
                                PairingUiStatus::Starting
                            } else {
                                PairingUiStatus::Waiting
                            }
                        }
                        None => PairingUiStatus::Failed,
                    };
                    self.screen = Screen::PairingStatus {
                        device_id: *device_id,
                        status,
                    };
                }
                _ => {}
            },
            Screen::PairingStatus { device_id, status } => match key {
                Key::Back | Key::Enter if *status == PairingUiStatus::Paired => {
                    self.screen = Screen::ChatsList { selected: 0 }
                }
                Key::Back | Key::Enter => {
                    self.screen = Screen::DiscoveredDevice {
                        device_id: *device_id,
                        selected: 1,
                    }
                }
                _ => {}
            },
            Screen::AnnouncementStatus { .. } => {
                if key == Key::Back || key == Key::Enter {
                    self.screen = Screen::ContactsList { selected: 0 };
                }
            }
            Screen::ChatView {
                conversation,
                unencrypted_dm,
                from_discovery,
                input,
                cursor,
                menu_open,
                menu_index,
                selected_message,
                ..
            } => {
                let rows = self.messages.len();
                match key {
                    Key::Back if *menu_open => *menu_open = false,
                    Key::Back if *from_discovery => {
                        if let ConversationId::Direct(device_id) = conversation {
                            self.screen = Screen::DiscoveredDevice {
                                device_id: *device_id,
                                selected: 0,
                            };
                        }
                    }
                    Key::Back => self.screen = Screen::ChatsList { selected: 0 },
                    Key::Left => *cursor = cursor.saturating_sub(1),
                    Key::Right => *cursor = (*cursor + 1).min(input.len()),
                    Key::Up if *menu_open => *menu_index = menu_index.saturating_sub(1),
                    Key::Down if *menu_open => *menu_index = (*menu_index + 1) % 2,
                    // Browse history rows only while nothing is being typed.
                    Key::Up if input.is_empty() && !*menu_open && rows > 0 => {
                        *selected_message = Some(match *selected_message {
                            None => rows - 1,
                            Some(picked) => (picked + rows - 1) % rows,
                        });
                    }
                    Key::Down if input.is_empty() && !*menu_open && rows > 0 => {
                        *selected_message = match *selected_message {
                            None => Some(0),
                            Some(picked) if picked + 1 < rows => Some(picked + 1),
                            Some(_) => None,
                        };
                    }
                    Key::Enter if *menu_open && can_send => {
                        let body = match *menu_index {
                            1 if self.gps_snapshot.fix => Some(OutgoingBody::Coords {
                                latitude_microdegrees: self.gps_snapshot.latitude_microdegrees,
                                longitude_microdegrees: self.gps_snapshot.longitude_microdegrees,
                            }),
                            _ => match input.is_empty() {
                                true => None,
                                false => Some(OutgoingBody::Text(input.clone())),
                            },
                        };
                        if let Some(body) = body {
                            self.pending_message = Some(OutgoingMessage {
                                conversation: *conversation,
                                unencrypted: match conversation {
                                    ConversationId::Broadcast => true,
                                    _ => *unencrypted_dm,
                                },
                                body,
                            });
                        }
                        *menu_open = false;
                    }
                    Key::Enter if can_send => *menu_open = true,
                    Key::Enter => {}
                    Key::Backspace if *cursor > 0 => {
                        input.remove(*cursor - 1);
                        *cursor -= 1;
                    }
                    Key::Character(ch) if ch.is_ascii() && !ch.is_control() => {
                        if input.push(ch).is_ok() {
                            *cursor = input.len();
                        }
                    }
                    _ => {}
                }
            }
            Screen::Navigation {
                conversation,
                unencrypted_dm,
                from_discovery,
                ..
            } => {
                if key == Key::Back {
                    let conversation = *conversation;
                    let unencrypted_dm = *unencrypted_dm;
                    let from_discovery = *from_discovery;
                    self.screen = chat_screen(conversation, unencrypted_dm, from_discovery);
                }
            }
            Screen::Settings { selected } => match key {
                Key::Back => self.screen = Screen::MainMenu { selected: 2 },
                Key::Up => *selected = (*selected + 2) % 3,
                Key::Down => *selected = (*selected + 1) % 3,
                Key::Enter => {
                    self.screen = Screen::SettingsSection {
                        section: SettingsSection::from_index(*selected),
                        selected: 0,
                    }
                }
                _ => {}
            },
            Screen::SettingsSection { section, selected } => match key {
                Key::Back => {
                    self.screen = Screen::Settings {
                        selected: section.root_index(),
                    }
                }
                Key::Up => {
                    *selected = (*selected + section.item_count() - 1) % section.item_count()
                }
                Key::Down => *selected = (*selected + 1) % section.item_count(),
                Key::Enter => {
                    self.screen = Screen::SettingsItem {
                        section: *section,
                        item: *selected,
                    }
                }
                _ => {}
            },
            Screen::SettingsItem { section, item } => match key {
                Key::Back => {
                    self.screen = Screen::SettingsSection {
                        section: *section,
                        selected: *item,
                    }
                }
                Key::Right | Key::Up
                    if section.is_timezone_item(*item) =>
                {
                    self.adjust_timezone_offset(crate::ui::display_settings::TIMEZONE_STEP)
                }
                Key::Left | Key::Down if section.is_timezone_item(*item) => {
                    self.adjust_timezone_offset(crate::ui::display_settings::TIMEZONE_STEP * -1)
                }
                _ => {}
            },
        }
    }

    /// Nudge the UTC offset by one step and queue the new value for flash.
    fn adjust_timezone_offset(&mut self, delta_minutes: i32) {
        let next = (self.timezone_offset_minutes + delta_minutes)
            .clamp(
                crate::ui::display_settings::MIN_TIMEZONE_OFFSET,
                crate::ui::display_settings::MAX_TIMEZONE_OFFSET,
            );
        if next != self.timezone_offset_minutes {
            self.timezone_offset_minutes = next;
            self.pending_timezone = Some(next);
        }
    }

    /// The UTC offset whose new value the run loop still has to persist.
    pub fn take_pending_timezone(&mut self) -> Option<i32> {
        self.pending_timezone.take()
    }

    /// Remember a peer the book has never seen so it appears in the list.
    /// Direct conversations only; broadcast is always present.
    pub fn note_unknown_sender(&mut self, conversation: ConversationId) {
        if let ConversationId::Direct(device_id) = conversation {
            self.contacts.note_unknown(device_id);
        }
    }

    /// Whether the open chat targets a contact with a pair key.
    fn active_chat_is_encrypted(&self) -> bool {
        match &self.screen {
            Screen::ChatView {
                conversation: ConversationId::Direct(device_id),
                ..
            } => self.contacts.direct_key(device_id).is_some(),
            _ => false,
        }
    }

    /// Number of CHATS rows: every visible contact plus the clear-all row.
    fn chat_rows(&self) -> usize {
        self.contacts.visible_indices(false).len() + 1
    }

    /// Conversation of one CHATS row, or `None` for the clear-all row.
    fn chat_row_kind(&self, row: usize) -> Option<ConversationId> {
        self.contacts
            .visible_indices(false)
            .get(row)
            .and_then(|index| self.contacts.entry(*index))
            .map(|entry| entry.kind)
    }


    /// Apply one 5x5 keypad key. Two hex digits type one ASCII character, so
    /// a lone digit stays pending until its pair arrives or BACK clears it.
    pub fn handle_keypad(&mut self, key: KeypadKey) {
        if key == KeypadKey::Help {
            self.help_open = !self.help_open;
            return;
        }
        // Any other keypad key hides the toast first, like the logical keys.
        if self.popup.is_some() {
            self.popup = None;
            return;
        }
        if self.help_open {
            if matches!(key, KeypadKey::Back | KeypadKey::Enter) {
                self.help_open = false;
            }
            return;
        }
        match key {
            KeypadKey::Digit(digit) => match &mut self.screen {
                Screen::ChatView {
                    pending_hex,
                    input,
                    cursor,
                    ..
                } => Self::type_hex_digit(pending_hex, input, cursor, digit),
                Screen::PairingEntry {
                    pending_hex,
                    secret,
                    cursor,
                    ..
                } => Self::type_hex_digit(pending_hex, secret, cursor, digit),
                _ => {}
            },
            KeypadKey::Back => self.handle_keypad_back(),
            KeypadKey::Send | KeypadKey::Enter => self.handle_key(Key::Enter),
            KeypadKey::Up => self.handle_key(Key::Up),
            KeypadKey::Down => self.handle_key(Key::Down),
            KeypadKey::Left => self.handle_key(Key::Left),
            KeypadKey::Right => self.handle_key(Key::Right),
            // Handled above, before the overlay gate.
            KeypadKey::Help | KeypadKey::Unassigned => {}
        }
    }

    /// Commit a hex pair to the focused field and leave a lone digit pending.
    fn type_hex_digit<const N: usize>(
        pending_hex: &mut Option<u8>,
        text: &mut String<N>,
        cursor: &mut usize,
        digit: u8,
    ) {
        let digit = digit & 0x0F;
        match pending_hex.take() {
            None => *pending_hex = Some(digit),
            Some(high) => {
                let character = char::from((high << 4) | digit);
                if character.is_ascii() && !character.is_control() && text.push(character).is_ok() {
                    *cursor = text.len();
                }
            }
        }
    }

    /// BACK clears a pending nibble, then deletes, and only then leaves the screen.
    fn handle_keypad_back(&mut self) {
        let position = match &self.screen {
            Screen::ChatView {
                pending_hex,
                cursor,
                ..
            } => (pending_hex.is_some(), *cursor),
            Screen::PairingEntry {
                pending_hex,
                cursor,
                ..
            } => (pending_hex.is_some(), *cursor),
            _ => {
                self.handle_key(Key::Back);
                return;
            }
        };
        let (pending_hex, cursor) = position;
        if pending_hex {
            match &mut self.screen {
                Screen::ChatView { pending_hex, .. } | Screen::PairingEntry { pending_hex, .. } => {
                    *pending_hex = None
                }
                _ => {}
            }
        } else if cursor > 0 {
            self.handle_key(Key::Backspace);
        } else {
            self.handle_key(Key::Back);
        }
    }

    /// Keep the latest fix and estimate course after at least five metres of movement.
    pub fn update_gps_snapshot(&mut self, snapshot: crate::ui::GpsSnapshot) {
        self.gps_snapshot = snapshot;
        if !snapshot.fix {
            self.course_anchor = None;
            self.movement_course_degrees = None;
            return;
        }

        let current = (
            snapshot.latitude_microdegrees,
            snapshot.longitude_microdegrees,
        );
        let Some(anchor) = self.course_anchor else {
            self.course_anchor = Some(current);
            return;
        };
        // Estimate course only after the user has moved far enough to reduce GPS drift.
        if let Some((distance, bearing)) =
            crate::ui::navigation::distance_and_bearing(anchor, current)
            && distance >= 5.0
        {
            self.movement_course_degrees = Some(bearing);
            self.course_anchor = Some(current);
        }
    }

    /// ENTER opens the picked coordinate row's navigation screen; `false` when
    /// nothing is picked or the row carries no position.
    fn open_selected_coordinate(&mut self) -> bool {
        let (conversation, unencrypted_dm, from_discovery, picked) = match &self.screen {
            Screen::ChatView {
                conversation,
                unencrypted_dm,
                from_discovery,
                input,
                menu_open,
                selected_message,
                ..
            } if input.is_empty() && !*menu_open => {
                (
                    *conversation,
                    *unencrypted_dm,
                    *from_discovery,
                    *selected_message,
                )
            }
            _ => return false,
        };
        let Some(picked) = picked else { return false };
        let Some(row) = self.messages.get(picked) else { return false };
        let Some((target_latitude_microdegrees, target_longitude_microdegrees)) = row.coordinates
        else {
            return false;
        };
        if !row.from_me && row.encrypted && !self.active_chat_is_encrypted() {
            return false;
        }
        self.screen = Screen::Navigation {
            conversation,
            unencrypted_dm,
            from_discovery,
            target_latitude_microdegrees,
            target_longitude_microdegrees,
        };
        true
    }

    pub fn take_pending_message(&mut self) -> Option<OutgoingMessage> {
        self.pending_message.take()
    }

    pub fn take_pending_manage(&mut self) -> Option<ManageCommand> {
        self.pending_manage.take()
    }

    pub fn take_pending_delete_all(&mut self) -> bool {
        core::mem::replace(&mut self.pending_delete_all, false)
    }

    /// The conversation whose chat panel is open, if any.
    pub fn active_conversation(&self) -> Option<ConversationId> {
        match &self.screen {
            Screen::ChatView { conversation, .. } => Some(*conversation),
            _ => None,
        }
    }

    /// Replace chat rows wholesale after a flash read.
    pub fn set_chat_rows(&mut self, rows: Vec<ChatRow, MAX_CHAT_MESSAGES>) {
        self.messages = rows;
    }

    /// Show one received-message toast over whatever screen is active.
    pub fn show_popup(&mut self, popup: crate::notify::Popup) {
        self.popup = Some(popup);
    }

    /// Clear the composer after a successful send.
    pub fn clear_chat_input(&mut self) {
        if let Screen::ChatView {
            input,
            cursor,
            pending_hex,
            ..
        } = &mut self.screen
        {
            input.clear();
            *cursor = 0;
            *pending_hex = None;
        }
    }

    /// Return to the list whose contact rows just changed, dropping stale picks.
    pub fn after_manage(&mut self, from_chats: bool) {
        self.screen = if from_chats {
            Screen::ChatsList { selected: 0 }
        } else {
            Screen::ContactsList { selected: 0 }
        };
    }

    pub fn take_pending_announcement(&mut self) -> bool {
        core::mem::replace(&mut self.pending_announcement, false)
    }

    pub fn take_pending_pairing(&mut self) -> Option<PairingCommand> {
        self.pending_pairing.take()
    }

    pub fn set_pairing_status(&mut self, device_id: [u8; 16], status: PairingUiStatus) {
        if let Screen::PairingStatus {
            device_id: current,
            status: current_status,
        } = &mut self.screen
            && *current == device_id
        {
            *current_status = status;
        }
    }

    pub fn handle_pairing_event(&mut self, event: PairingEvent) {
        if event.succeeded {
            let contact = Contact {
                device_id: event.peer_id,
                key: event.key,
                custom_name: heapless::String::new(),
                blocked: false,
            };
            self.paired_contact(&contact);
        } else {
            self.set_pairing_status(event.peer_id, PairingUiStatus::Failed);
        }
    }

    /// Insert or update the contact for a successful pairing.
    pub fn paired_contact(&mut self, contact: &Contact) {
        let _ = self.contacts.adopt(contact);
        self.discovered_devices
            .retain(|device_id| *device_id != contact.device_id);
        self.set_pairing_status(contact.device_id, PairingUiStatus::Paired);
    }

    pub fn set_announcement_status(&mut self, status: AnnouncementStatus) {
        if matches!(self.screen, Screen::AnnouncementStatus { .. }) {
            self.screen = Screen::AnnouncementStatus { status };
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
            0,
        )
    }
}

/// One chat row exactly as the four-line panel draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRow {
    pub from_me: bool,
    pub encrypted: bool,
    pub acknowledged: bool,
    pub coordinates: Option<(i32, i32)>,
    /// Already truncated to `ROW_TEXT_CHARS`; empty for a coordinate row.
    pub text: String<ROW_TEXT_CHARS>,
}

impl ChatRow {
    /// Trim one flash record down to the fields the panel draws.
    pub fn from_stored(message: &crate::storage::message::StoredMessage) -> Self {
        let mut row = Self {
            from_me: message.from_me,
            encrypted: message.encrypted,
            acknowledged: message.acknowledged,
            coordinates: match &message.kind {
                MessageKind::Text => None,
                MessageKind::Coords {
                    latitude_microdegrees,
                    longitude_microdegrees,
                } => Some((*latitude_microdegrees, *longitude_microdegrees)),
            },
            text: String::new(),
        };
        let source = match &message.kind {
            MessageKind::Text => message.text.as_str(),
            MessageKind::Coords { .. } => "[GPS]",
        };
        let mut preview = String::<PREVIEW_CHARS>::new();
        let _ = preview.push_str(source);
        for character in preview.chars().take(ROW_TEXT_CHARS) {
            if row.text.push(character).is_err() {
                break;
            }
        }
        row
    }
}
