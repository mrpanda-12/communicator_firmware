use heapless::{String, Vec};

use crate::{
    crypto::{self, KEY_SIZE},
    network::{
        discovery::DiscoveryList,
        message_delivery::{IncomingMessage, MESSAGE_TEXT_CAPACITY},
        pairing::PairingEvent,
    },
    radio::types::RadioConfig,
};

pub use super::contact::ConversationKind;
use super::contact::{ContactBook, ConversationMessage};

const MAX_CHAT_MESSAGES: usize = 4;

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
            Self::Gps => 2,
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
            Self::Gps => 3,
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
            (Self::Gps, _) => "UTC TIME",
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
            (Self::Gps, _) => "GPS time is shown in UTC when available.",
        }
    }
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
    },
    PairingStatus {
        device_id: [u8; 16],
        status: PairingUiStatus,
    },
    AnnouncementStatus {
        status: AnnouncementStatus,
    },
    ChatView {
        conversation: ConversationKind,
        unencrypted_dm: bool,
        from_discovery: bool,
        selected_message: usize,
        input: String<MESSAGE_TEXT_CAPACITY>,
        cursor: usize,
        menu_open: bool,
    },
    Navigation {
        conversation: ConversationKind,
        unencrypted_dm: bool,
        from_discovery: bool,
        selected_message: usize,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutgoingMessage {
    pub to: [u8; 16],
    pub text: String<MESSAGE_TEXT_CAPACITY>,
    pub unencrypted: bool,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub screen: Screen,
    pub contacts: ContactBook,
    pub device_id: Option<[u8; 16]>,
    pub firmware_version: &'static str,
    pub radio_config: RadioConfig,
    pub discovered_devices: DiscoveryList,
    pub messages: Vec<ConversationMessage, MAX_CHAT_MESSAGES>,
    pub gps_snapshot: crate::ui::GpsSnapshot,
    pub movement_course_degrees: Option<u16>,
    course_anchor: Option<(i32, i32)>,
    pending_message: Option<OutgoingMessage>,
    pending_announcement: bool,
    pending_pairing: Option<PairingCommand>,
}

impl UiState {
    /// 1/ Start at the application menu with chats selected.
    pub fn new(
        device_id: Option<[u8; 16]>,
        contacts: ContactBook,
        firmware_version: &'static str,
        radio_config: RadioConfig,
        discovered_devices: DiscoveryList,
    ) -> Self {
        Self {
            screen: Screen::MainMenu { selected: 0 },
            contacts,
            device_id,
            firmware_version,
            radio_config,
            discovered_devices,
            messages: Vec::new(),
            gps_snapshot: crate::ui::GpsSnapshot::default(),
            movement_course_degrees: None,
            course_anchor: None,
            pending_message: None,
            pending_announcement: false,
            pending_pairing: None,
        }
    }

    /// 2/ Apply a logical key event; the input hardware can map buttons to this API.
    pub fn handle_key(&mut self, key: Key) {
        if key == Key::Enter && self.open_selected_coordinate() {
            return;
        }
        let encrypted_direct = match &self.screen {
            Screen::ChatView {
                conversation: ConversationKind::Direct(device_id),
                ..
            } => self.contacts.direct_key(device_id).is_some(),
            _ => false,
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
            Screen::ChatsList { selected } => match key {
                Key::Back => self.screen = Screen::MainMenu { selected: 0 },
                Key::Up if *selected > 0 => *selected -= 1,
                Key::Down if *selected + 1 < self.contacts.entries().len() => *selected += 1,
                Key::Enter if self.contacts.get(*selected).is_some() => {
                    let conversation = self.contacts.get(*selected).unwrap().kind;
                    if let ConversationKind::Direct(device_id) = conversation
                        && self.contacts.direct_key(&device_id).is_none()
                    {
                        self.screen = Screen::UnencryptedDmWarning {
                            device_id,
                            from_discovery: false,
                        };
                    } else {
                        self.screen = Screen::ChatView {
                            conversation,
                            unencrypted_dm: false,
                            from_discovery: false,
                            selected_message: 0,
                            input: String::new(),
                            cursor: 0,
                            menu_open: false,
                        };
                    }
                }
                Key::Enter => {}
                _ => {}
            },
            Screen::ContactsList { selected } => {
                let direct_count = self.contacts.entries().len().saturating_sub(1);
                let nearby_index = direct_count;
                let scan_index = direct_count + 1;
                let item_count = direct_count + 2;
                match key {
                    Key::Back => self.screen = Screen::MainMenu { selected: 1 },
                    Key::Up => *selected = (*selected + item_count - 1) % item_count,
                    Key::Down => *selected = (*selected + 1) % item_count,
                    Key::Enter if *selected == nearby_index => {
                        self.screen = Screen::DeviceSearch { selected: 0 }
                    }
                    Key::Enter if *selected == scan_index => {
                        self.pending_announcement = true;
                        self.screen = Screen::DeviceSearch { selected: 0 };
                    }
                    Key::Enter => {
                        if let Some(contact) = self.contacts.get(*selected + 1) {
                            let conversation = contact.kind;
                            if let ConversationKind::Direct(device_id) = conversation
                                && self.contacts.direct_key(&device_id).is_none()
                            {
                                self.screen = Screen::UnencryptedDmWarning {
                                    device_id,
                                    from_discovery: false,
                                };
                            } else {
                                self.screen = Screen::ChatView {
                                    conversation,
                                    unencrypted_dm: false,
                                    from_discovery: false,
                                    selected_message: 0,
                                    input: String::new(),
                                    cursor: 0,
                                    menu_open: false,
                                };
                            }
                        }
                    }
                    _ => {}
                }
            }
            Screen::DeviceSearch { selected } => {
                let item_count = self.discovered_devices.len().max(1);
                match key {
                    Key::Back => {
                        self.screen = Screen::ContactsList {
                            selected: self.contacts.entries().len().saturating_sub(1),
                        }
                    }
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
                    self.screen = Screen::ChatView {
                        conversation: ConversationKind::Direct(*device_id),
                        unencrypted_dm: true,
                        from_discovery: *from_discovery,
                        selected_message: 0,
                        input: String::new(),
                        cursor: 0,
                        menu_open: false,
                    }
                }
                _ => {}
            },
            Screen::PairingEntry {
                device_id,
                secret,
                cursor,
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
                            let key = crypto::derive_pair_key(secret.as_bytes(), &my_id, device_id);
                            self.pending_pairing = Some(PairingCommand {
                                peer_id: *device_id,
                                key,
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
                    self.screen = Screen::ContactsList {
                        selected: self.contacts.entries().len().saturating_sub(1),
                    }
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
                    self.screen = Screen::ContactsList {
                        selected: self.contacts.entries().len(),
                    };
                }
            }
            Screen::ChatView {
                conversation,
                unencrypted_dm,
                from_discovery,
                selected_message,
                input,
                cursor,
                menu_open,
            } => match key {
                Key::Back if *menu_open => *menu_open = false,
                Key::Back if *from_discovery => {
                    if let ConversationKind::Direct(device_id) = conversation {
                        self.screen = Screen::DiscoveredDevice {
                            device_id: *device_id,
                            selected: 0,
                        };
                    }
                }
                Key::Back => self.screen = Screen::ChatsList { selected: 0 },
                Key::Up => *selected_message = selected_message.saturating_sub(1),
                Key::Down => {
                    let peer_id = match conversation {
                        ConversationKind::Broadcast => [0; 16],
                        ConversationKind::Direct(device_id) => *device_id,
                    };
                    let visible_count = self
                        .messages
                        .iter()
                        .filter(|message| message.peer_id == peer_id)
                        .count()
                        .min(4);
                    *selected_message =
                        (*selected_message + 1).min(visible_count.saturating_sub(1));
                }
                Key::Left => *cursor = cursor.saturating_sub(1),
                Key::Right => *cursor = (*cursor + 1).min(input.len()),
                Key::Enter
                    if *menu_open && (*unencrypted_dm || encrypted_direct) && !input.is_empty() =>
                {
                    if let ConversationKind::Direct(to) = conversation {
                        self.pending_message = Some(OutgoingMessage {
                            to: *to,
                            text: input.clone(),
                            unencrypted: *unencrypted_dm,
                        });
                    }
                    *menu_open = false;
                }
                Key::Enter if (*unencrypted_dm || encrypted_direct) && !input.is_empty() => {
                    *menu_open = true
                }
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
            },
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
                _ => {}
            },
            Screen::Navigation {
                conversation,
                unencrypted_dm,
                from_discovery,
                selected_message,
                ..
            } => match key {
                Key::Back => {
                    self.screen = Screen::ChatView {
                        conversation: *conversation,
                        unencrypted_dm: *unencrypted_dm,
                        from_discovery: *from_discovery,
                        selected_message: *selected_message,
                        input: String::new(),
                        cursor: 0,
                        menu_open: false,
                    };
                }
                _ => {}
            },
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

    fn open_selected_coordinate(&mut self) -> bool {
        // Open only a coordinate in the same visible four-row order used by the chat renderer.
        let (conversation, unencrypted_dm, from_discovery, selected_message) = match &self.screen {
            Screen::ChatView {
                conversation,
                unencrypted_dm,
                from_discovery,
                selected_message,
                input,
                menu_open,
                ..
            } if input.is_empty() && !*menu_open => (
                *conversation,
                *unencrypted_dm,
                *from_discovery,
                *selected_message,
            ),
            _ => return false,
        };
        let peer_id = match conversation {
            ConversationKind::Broadcast => [0; 16],
            ConversationKind::Direct(device_id) => device_id,
        };
        let mut visible = Vec::<&ConversationMessage, MAX_CHAT_MESSAGES>::new();
        for message in self
            .messages
            .iter()
            .rev()
            .filter(|message| message.peer_id == peer_id)
            .take(MAX_CHAT_MESSAGES)
        {
            let _ = visible.push(message);
        }
        let target = visible
            .iter()
            .rev()
            .nth(selected_message)
            .and_then(|message| message.coordinates);
        let Some((target_latitude_microdegrees, target_longitude_microdegrees)) = target else {
            return false;
        };
        self.screen = Screen::Navigation {
            conversation,
            unencrypted_dm,
            from_discovery,
            selected_message,
            target_latitude_microdegrees,
            target_longitude_microdegrees,
        };
        true
    }

    pub fn take_pending_message(&mut self) -> Option<OutgoingMessage> {
        self.pending_message.take()
    }

    pub fn record_incoming_message(&mut self, incoming: IncomingMessage) {
        match incoming {
            IncomingMessage::Text {
                peer_id,
                sender_id,
                message_id,
                text,
                encrypted,
            } => {
                if peer_id != [0; 16] && !self.contacts.contains_direct(&peer_id) {
                    let _ = self.contacts.add_direct(peer_id, "");
                }
                self.push_conversation_message(ConversationMessage {
                    peer_id,
                    sender_id,
                    message_id,
                    from_local: false,
                    encrypted,
                    acknowledged: false,
                    coordinates: None,
                    text,
                });
            }
            IncomingMessage::Coordinates {
                peer_id,
                sender_id,
                message_id,
                latitude_microdegrees,
                longitude_microdegrees,
                encrypted,
            } => {
                if peer_id != [0; 16] && !self.contacts.contains_direct(&peer_id) {
                    let _ = self.contacts.add_direct(peer_id, "");
                }
                self.push_conversation_message(ConversationMessage {
                    peer_id,
                    sender_id,
                    message_id,
                    from_local: false,
                    encrypted,
                    acknowledged: false,
                    coordinates: Some((latitude_microdegrees, longitude_microdegrees)),
                    text: String::new(),
                });
            }
            IncomingMessage::Acknowledgement {
                peer_id,
                message_id,
            } => {
                if let Some(message) = self.messages.iter_mut().rev().find(|message| {
                    message.peer_id == peer_id
                        && message.message_id == message_id
                        && message.from_local
                        && message.encrypted
                        && peer_id != [0; 16]
                }) {
                    message.acknowledged = true;
                }
            }
        }
    }

    pub fn record_outgoing_message(
        &mut self,
        peer_id: [u8; 16],
        sender_id: [u8; 16],
        message_id: u16,
        text: &str,
        encrypted: bool,
    ) {
        if !self.contacts.contains_direct(&peer_id) {
            let _ = self.contacts.add_direct(peer_id, "");
        }
        let mut message_text = String::new();
        if message_text.push_str(text).is_ok() {
            self.push_conversation_message(ConversationMessage {
                peer_id,
                sender_id,
                message_id,
                from_local: true,
                encrypted,
                acknowledged: false,
                coordinates: None,
                text: message_text,
            });
        }
    }

    fn push_conversation_message(&mut self, message: ConversationMessage) {
        if self.messages.is_full() {
            self.messages.remove(0);
        }
        let _ = self.messages.push(message);
    }

    pub fn clear_chat_input(&mut self) {
        if let Screen::ChatView { input, cursor, .. } = &mut self.screen {
            input.clear();
            *cursor = 0;
        }
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
            let _ = self
                .contacts
                .add_paired_direct(event.peer_id, "", event.key);
            self.discovered_devices
                .retain(|device_id| *device_id != event.peer_id);
            self.set_pairing_status(event.peer_id, PairingUiStatus::Paired);
        } else {
            self.set_pairing_status(event.peer_id, PairingUiStatus::Failed);
        }
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
        )
    }
}
