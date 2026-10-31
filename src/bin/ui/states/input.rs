//! Logical key handling: what every key does on every screen.

use heapless::String;

use crate::crypto;
use crate::storage::message::ConversationId;

use crate::ui::states::settings::TIMEZONE_STEP;

use super::{
    DeviceOrigin, FREQUENCY_PRESETS, FrequencyUiStatus, HistoryPageRequest, Key, MAX_CHAT_MESSAGES,
    ManageAction, ManageCommand, OutgoingBody, OutgoingMessage, PairingCommand, PairingUiStatus,
    RenameCommand, ResetUiStatus, Screen, SettingsSection, UiState, device_list,
};

/// Rows on the settings menu: three categories, the block list, and the
/// delete-all confirmation.
const SETTINGS_ROWS: usize = 5;
/// Settings row that opens the block list.
const SETTINGS_BLOCKED_ROW: usize = 3;

/// The screen a chat entry opens: the warning when a direct chat has no key,
/// the chat view itself otherwise.
fn open_chat_target(
    device_has_key: bool,
    conversation: ConversationId,
    from_discovery: bool,
) -> Screen {
    if let ConversationId::Direct(device_id) = conversation
        && !device_has_key
    {
        return Screen::UnencryptedDmWarning {
            device_id,
            from_discovery,
        };
    }
    chat_screen(conversation, false, from_discovery)
}

fn chat_screen(conversation: ConversationId, unencrypted_dm: bool, from_discovery: bool) -> Screen {
    Screen::ChatView {
        conversation,
        unencrypted_dm,
        from_discovery,
        cursor: 0,
        pending_hex: None,
        menu_open: false,
        menu_index: 0,
        selected_message: None,
        history_base: 0,
        history_start: 0,
        history_total: 0,
    }
}

impl UiState {
    /// Apply a logical key event; the input hardware can map buttons to this API.
    pub fn handle_key(&mut self, key: Key) {
        // ENTER accepts (or acknowledges) a toast; BACK dismisses it.
        if let Some(popup_type) = self.popup.as_ref().map(|popup| popup.popup_type) {
            match (popup_type, key) {
                (crate::notify::PopupType::FrequencyChangeRequest { frequency_hz }, Key::Enter) => {
                    self.pending_frequency = Some(frequency_hz);
                    self.dismiss_popup();
                }
                (_, Key::Enter | Key::Back) => self.dismiss_popup(),
                _ => {}
            }
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
            Screen::ChatView { unencrypted_dm, .. } => *unencrypted_dm || encrypted_direct,
            _ => false,
        };
        // This lookup runs before the mutable screen match, because a helper
        // method inside its arms would borrow the whole state.
        let chats_row_conversation = match &self.screen {
            Screen::ChatsList { selected } => self.chat_row_conversation(*selected),
            _ => None,
        };
        let contacts_open_conversation: Option<ConversationId> = match &self.screen {
            Screen::ContactsList { selected } => {
                let indices = self.contacts.visible_indices(true);
                indices
                    .get(*selected)
                    .and_then(|index| self.contacts.entry(*index))
                    .map(|entry| entry.conversation)
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
                let row_conversation = chats_row_conversation;
                let row_count = self.contacts.chat_indices().len();
                match key {
                    Key::Back => self.screen = Screen::MainMenu { selected: 0 },
                    // Both directions wrap, like every other list here.
                    Key::Up if row_count > 0 => *selected = (*selected + row_count - 1) % row_count,
                    Key::Down if row_count > 0 => *selected = (*selected + 1) % row_count,
                    Key::Enter => {
                        if let Some(conversation) = row_conversation {
                            let has_key = match conversation {
                                ConversationId::Direct(device_id) => {
                                    self.contacts.direct_key(&device_id).is_some()
                                }
                                ConversationId::Broadcast => true,
                            };
                            self.chat_input.clear();
                            self.screen = open_chat_target(has_key, conversation, false);
                        }
                    }
                    // RIGHT opens the delete / block actions of that contact.
                    Key::Right => {
                        if let Some(ConversationId::Direct(device_id)) = row_conversation {
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
                let add_index = indices.len();
                let devices_index = add_index + 1;
                let item_count = indices.len() + 2;
                match key {
                    Key::Back => self.screen = Screen::MainMenu { selected: 1 },
                    Key::Up => *selected = (*selected + item_count - 1) % item_count,
                    Key::Down => *selected = (*selected + 1) % item_count,
                    Key::Right => {
                        if let Some(device_id) = indices
                            .get(*selected)
                            .and_then(|index| self.contacts.entry(*index))
                            .and_then(|entry| match entry.conversation {
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
                    // ADD DEVICE announces every two seconds and lists replies.
                    Key::Enter if *selected == add_index => {
                        self.pending_announcement = true;
                        self.screen = Screen::AnnounceDevices { selected: 0 };
                    }
                    // DEVICES shows active announcements, then passive peers.
                    Key::Enter if *selected == devices_index => {
                        self.screen = Screen::Devices { selected: 0 };
                    }
                    Key::Enter => {
                        if let Some(conversation) = contacts_open_conversation {
                            let has_key = match conversation {
                                ConversationId::Direct(device_id) => {
                                    self.contacts.direct_key(&device_id).is_some()
                                }
                                ConversationId::Broadcast => true,
                            };
                            self.chat_input.clear();
                            self.screen = open_chat_target(has_key, conversation, false);
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
                    .entry_by_conversation(ConversationId::Direct(*device_id))
                    .is_some();
                let entry = self
                    .contacts
                    .entry_by_conversation(ConversationId::Direct(*device_id));
                let currently_blocked = entry.is_some_and(|entry| entry.blocked);
                let name = entry
                    .map(|entry| entry.custom_name.clone())
                    .unwrap_or_default();
                let action_count = 5;
                match key {
                    Key::Back => {
                        self.screen = if *from_chats {
                            Screen::ChatsList { selected: 0 }
                        } else {
                            Screen::ContactsList { selected: 0 }
                        };
                    }
                    Key::Up => *selected = (*selected + action_count - 1) % action_count,
                    Key::Down => *selected = (*selected + 1) % action_count,
                    Key::Enter if have_entry && *selected == 0 => {
                        self.screen = Screen::NameEntry {
                            device_id: *device_id,
                            cursor: name.len(),
                            name,
                            pending_hex: None,
                            add_flow: false,
                            from_chats: *from_chats,
                            origin: DeviceOrigin::Recent,
                        };
                    }
                    Key::Enter if have_entry && *selected == 1 => {
                        self.pending_rename = Some(RenameCommand {
                            device_id: *device_id,
                            name: String::new(),
                        });
                        self.screen = if *from_chats {
                            Screen::ChatsList { selected: 0 }
                        } else {
                            Screen::ContactsList { selected: 0 }
                        };
                    }
                    Key::Enter if have_entry && *selected == 2 => {
                        self.screen = Screen::SecretEntry {
                            device_id: *device_id,
                            name,
                            secret: String::new(),
                            cursor: 0,
                            pending_hex: None,
                            from_manage: true,
                            origin: DeviceOrigin::Recent,
                        };
                    }
                    Key::Enter if have_entry => {
                        let action = match *selected {
                            3 => ManageAction::Block(!currently_blocked),
                            _ => ManageAction::Delete,
                        };
                        self.pending_manage = Some(ManageCommand {
                            device_id: *device_id,
                            action,
                            from_chats: *from_chats,
                            from_blocked: false,
                        });
                    }
                    _ => {}
                }
            }
            Screen::AnnounceDevices { selected } => {
                let item_count = self.discovered_devices.len().max(1);
                match key {
                    // BACK is the universal exit: it also stops the repeats.
                    Key::Back => {
                        self.cancel_scan = true;
                        self.screen = Screen::ContactsList { selected: 0 }
                    }
                    Key::Up => *selected = (*selected + item_count - 1) % item_count,
                    Key::Down => *selected = (*selected + 1) % item_count,
                    Key::Enter => {
                        if let Some(device_id) = self.discovered_devices.get(*selected) {
                            self.screen = Screen::DiscoveredDevice {
                                device_id: *device_id,
                                selected: 0,
                                origin: DeviceOrigin::Announce,
                            };
                        }
                    }
                    _ => {}
                }
            }
            // One list: active announcements first, then passive traffic.
            Screen::Devices { selected } => {
                let active_len = self.announced_devices.len();
                let item_count = (active_len + self.passive_devices.len()).max(1);
                match key {
                    Key::Back => self.screen = Screen::ContactsList { selected: 0 },
                    Key::Up => *selected = (*selected + item_count - 1) % item_count,
                    Key::Down => *selected = (*selected + 1) % item_count,
                    Key::Enter => {
                        let pick = if *selected < active_len {
                            self.announced_devices
                                .get(*selected)
                                .map(|device_id| (*device_id, DeviceOrigin::Search))
                        } else {
                            self.passive_devices
                                .get(*selected - active_len)
                                .map(|device_id| (*device_id, DeviceOrigin::Recent))
                        };
                        if let Some((device_id, origin)) = pick {
                            self.screen = Screen::DiscoveredDevice {
                                device_id,
                                selected: 0,
                                origin,
                            };
                        }
                    }
                    _ => {}
                }
            }
            Screen::DiscoveredDevice {
                device_id,
                selected,
                origin,
            } => {
                // Both sections can add the peer or ask it to add us back.
                let item_count = 2;
                match key {
                    Key::Back => self.screen = device_list(*origin),
                    Key::Up => *selected = (*selected + item_count - 1) % item_count,
                    Key::Down => *selected = (*selected + 1) % item_count,
                    Key::Enter if *selected == 0 => {
                        self.screen = Screen::NameEntry {
                            device_id: *device_id,
                            name: String::new(),
                            cursor: 0,
                            pending_hex: None,
                            add_flow: true,
                            from_chats: false,
                            origin: *origin,
                        }
                    }
                    Key::Enter => {
                        self.pending_contact_request = Some(*device_id);
                        self.screen = device_list(*origin);
                    }
                    _ => {}
                }
            }
            Screen::UnencryptedDmWarning {
                device_id,
                from_discovery,
            } => match key {
                Key::Back if *from_discovery => {
                    self.screen = Screen::DiscoveredDevice {
                        device_id: *device_id,
                        selected: 0,
                        origin: DeviceOrigin::Recent,
                    }
                }
                Key::Back => self.screen = Screen::ChatsList { selected: 0 },
                Key::Enter => {
                    self.chat_input.clear();
                    self.screen =
                        chat_screen(ConversationId::Direct(*device_id), true, *from_discovery);
                }
                _ => {}
            },
            Screen::NameEntry {
                device_id,
                name,
                cursor,
                add_flow,
                from_chats,
                origin,
                ..
            } => match key {
                Key::Back => {
                    self.screen = if *add_flow {
                        Screen::DiscoveredDevice {
                            device_id: *device_id,
                            selected: 0,
                            origin: *origin,
                        }
                    } else {
                        Screen::ContactManage {
                            device_id: *device_id,
                            selected: 0,
                            from_chats: *from_chats,
                        }
                    }
                }
                Key::Left => *cursor = cursor.saturating_sub(1),
                Key::Right => *cursor = (*cursor + 1).min(name.len()),
                Key::Backspace if *cursor > 0 => {
                    name.remove(*cursor - 1);
                    *cursor -= 1;
                }
                Key::Character(ch) if ch.is_ascii() && !ch.is_control() => {
                    if name.push(ch).is_ok() {
                        *cursor = name.len();
                    }
                }
                Key::Enter if *add_flow => {
                    self.screen = Screen::SecretEntry {
                        device_id: *device_id,
                        name: name.clone(),
                        secret: String::new(),
                        cursor: 0,
                        pending_hex: None,
                        from_manage: false,
                        origin: *origin,
                    };
                }
                Key::Enter => {
                    let from_chats = *from_chats;
                    self.pending_rename = Some(RenameCommand {
                        device_id: *device_id,
                        name: name.clone(),
                    });
                    self.screen = if from_chats {
                        Screen::ChatsList { selected: 0 }
                    } else {
                        Screen::ContactsList { selected: 0 }
                    };
                }
                _ => {}
            },
            Screen::SecretEntry {
                device_id,
                name,
                secret,
                cursor,
                from_manage,
                origin,
                ..
            } => match key {
                Key::Back => {
                    self.screen = if *from_manage {
                        Screen::ContactManage {
                            device_id: *device_id,
                            selected: 2,
                            from_chats: false,
                        }
                    } else {
                        Screen::NameEntry {
                            device_id: *device_id,
                            name: name.clone(),
                            cursor: name.len(),
                            pending_hex: None,
                            add_flow: true,
                            from_chats: false,
                            origin: *origin,
                        }
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
                Key::Enter if !secret.is_empty() => {
                    let status = match self.device_id {
                        Some(my_id) => {
                            let derived =
                                crypto::derive_pair_key(secret.as_bytes(), &my_id, device_id);
                            self.pending_pairing = Some(PairingCommand {
                                peer_id: *device_id,
                                key: derived,
                            });
                            self.pending_contact_name = name.clone();
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
                // ENTER on an empty password adds the peer without a pair key.
                Key::Enter if secret.is_empty() => {
                    self.pending_rename = Some(RenameCommand {
                        device_id: *device_id,
                        name: name.clone(),
                    });
                    self.contacts.note_unknown(*device_id);
                    self.contacts.set_name(*device_id, name.as_str());
                    self.screen = Screen::ContactsList { selected: 0 };
                }
                _ => {}
            },
            Screen::PairingStatus { status, .. } => match key {
                Key::Back | Key::Enter if *status == PairingUiStatus::Paired => {
                    self.screen = Screen::ChatsList { selected: 0 }
                }
                Key::Back | Key::Enter => self.screen = Screen::ContactsList { selected: 0 },
                _ => {}
            },
            Screen::ChatView {
                conversation,
                unencrypted_dm,
                from_discovery,
                cursor,
                menu_open,
                menu_index,
                selected_message,
                history_base,
                history_start,
                history_total,
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
                                origin: DeviceOrigin::Recent,
                            };
                        }
                    }
                    Key::Back => self.screen = Screen::ChatsList { selected: 0 },
                    Key::Left => *cursor = cursor.saturating_sub(1),
                    Key::Right => *cursor = (*cursor + 1).min(self.chat_input.len()),
                    Key::Up if *menu_open => *menu_index = (*menu_index + 1) % 2,
                    Key::Down if *menu_open => *menu_index = (*menu_index + 1) % 2,
                    // Browse and page through flash history only while not typing.
                    Key::Up if self.chat_input.is_empty() && !*menu_open && rows > 0 => {
                        match *selected_message {
                            None => *selected_message = Some(rows - 1),
                            Some(picked) if picked > 0 => *selected_message = Some(picked - 1),
                            Some(_) if *history_start > *history_base => {
                                self.pending_history_page = Some(HistoryPageRequest {
                                    conversation: *conversation,
                                    start: Some(
                                        history_start.saturating_sub(MAX_CHAT_MESSAGES as u32),
                                    ),
                                    select: Some(history_start.saturating_sub(1)),
                                });
                            }
                            _ => {}
                        }
                    }
                    Key::Down if self.chat_input.is_empty() && !*menu_open && rows > 0 => {
                        match *selected_message {
                            None => *selected_message = Some(0),
                            Some(picked) if picked + 1 < rows => {
                                *selected_message = Some(picked + 1);
                            }
                            Some(_)
                                if history_start.saturating_add(rows as u32) < *history_total =>
                            {
                                let next = history_start.saturating_add(rows as u32);
                                self.pending_history_page = Some(HistoryPageRequest {
                                    conversation: *conversation,
                                    start: Some(next),
                                    select: Some(next),
                                });
                            }
                            Some(_) => *selected_message = None,
                        }
                    }
                    Key::Enter if *menu_open && can_send => {
                        let body = if *menu_index == 1 && self.gps_snapshot.fix {
                            Some(OutgoingBody::Coords {
                                latitude_microdegrees: self.gps_snapshot.latitude_microdegrees,
                                longitude_microdegrees: self.gps_snapshot.longitude_microdegrees,
                            })
                        } else if self.chat_input.is_empty() {
                            None
                        } else {
                            Some(OutgoingBody::Text)
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
                    // SEND transmits at once; it never opens a menu. The
                    // highlighted menu row wins while the menu is open,
                    // otherwise the typed text goes first, then the position.
                    Key::Send if can_send => {
                        let coordinates = *menu_open && *menu_index == 1;
                        let body = if coordinates && self.gps_snapshot.fix {
                            Some(OutgoingBody::Coords {
                                latitude_microdegrees: self.gps_snapshot.latitude_microdegrees,
                                longitude_microdegrees: self.gps_snapshot.longitude_microdegrees,
                            })
                        } else if self.chat_input.is_empty() {
                            None
                        } else {
                            Some(OutgoingBody::Text)
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
                    Key::Send => {}
                    Key::Backspace if *cursor > 0 => {
                        self.chat_input.remove(*cursor - 1);
                        *cursor -= 1;
                    }
                    Key::Character(ch) if ch.is_ascii() && !ch.is_control() => {
                        // A full composer drops the character; the cursor stays put.
                        let accepted = self.chat_input.push(ch).is_ok();
                        if accepted {
                            *cursor = self.chat_input.len();
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
                    self.chat_input.clear();
                    self.screen = chat_screen(conversation, unencrypted_dm, from_discovery);
                }
            }
            Screen::Settings { selected } => match key {
                Key::Back => self.screen = Screen::MainMenu { selected: 2 },
                Key::Up => *selected = (*selected + SETTINGS_ROWS - 1) % SETTINGS_ROWS,
                Key::Down => *selected = (*selected + 1) % SETTINGS_ROWS,
                Key::Enter if *selected == SETTINGS_BLOCKED_ROW => {
                    self.screen = Screen::BlockedDevices { selected: 0 }
                }
                // The last row asks before clearing every conversation.
                Key::Enter if *selected == SETTINGS_ROWS - 1 => {
                    self.screen = Screen::DeleteAllChats
                }
                Key::Enter => {
                    self.screen = Screen::SettingsSection {
                        section: SettingsSection::from_index(*selected),
                        selected: 0,
                    }
                }
                _ => {}
            },
            Screen::DeleteAllChats => match key {
                Key::Back => {
                    self.screen = Screen::Settings {
                        selected: SETTINGS_ROWS - 1,
                    }
                }
                Key::Enter => {
                    self.pending_delete_all = true;
                    self.screen = Screen::Settings {
                        selected: SETTINGS_ROWS - 1,
                    };
                }
                _ => {}
            },
            Screen::BlockedDevices { selected } => {
                let item_count = self.blocked.len().max(1);
                match key {
                    Key::Back => {
                        self.screen = Screen::Settings {
                            selected: SETTINGS_ROWS - 1,
                        }
                    }
                    Key::Up => *selected = (*selected + item_count - 1) % item_count,
                    Key::Down => *selected = (*selected + 1) % item_count,
                    // ENTER unblocks the picked device; the run loop saves it.
                    Key::Enter => {
                        if let Some(device_id) = self.blocked.get(*selected).copied() {
                            self.pending_manage = Some(ManageCommand {
                                device_id,
                                action: ManageAction::Block(false),
                                from_chats: false,
                                from_blocked: true,
                            });
                        }
                    }
                    _ => {}
                }
            }
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
                Key::Enter if *section == SettingsSection::Device && *item == 2 => {
                    self.pending_reset_identity = true;
                    self.screen = Screen::IdentityReset {
                        status: ResetUiStatus::Working,
                    };
                }
                Key::Enter if *section == SettingsSection::Frequency && *item == 1 => {
                    self.screen = Screen::FrequencyChange { selected: 0 };
                }
                Key::Right | Key::Up if section.is_timezone_item(*item) => {
                    self.adjust_timezone_offset(TIMEZONE_STEP)
                }
                Key::Left | Key::Down if section.is_timezone_item(*item) => {
                    self.adjust_timezone_offset(-TIMEZONE_STEP)
                }
                _ => {}
            },
            Screen::IdentityReset { .. } => {
                if key == Key::Back || key == Key::Enter {
                    self.screen = Screen::SettingsSection {
                        section: SettingsSection::Device,
                        selected: 2,
                    };
                }
            }
            Screen::FrequencyChange { selected } => match key {
                Key::Back => {
                    self.screen = Screen::SettingsItem {
                        section: SettingsSection::Frequency,
                        item: 1,
                    }
                }
                Key::Up => {
                    *selected = (*selected + FREQUENCY_PRESETS.len() - 1) % FREQUENCY_PRESETS.len()
                }
                Key::Down => *selected = (*selected + 1) % FREQUENCY_PRESETS.len(),
                Key::Enter => {
                    self.screen = Screen::FrequencyPeers {
                        frequency_hz: FREQUENCY_PRESETS[*selected].0,
                        cursor: 0,
                        chosen: 0,
                    };
                }
                _ => {}
            },
            Screen::FrequencyPeers {
                frequency_hz,
                cursor,
                chosen,
            } => {
                let peers = self.contacts.visible_indices(true).len();
                let row_count = peers.max(1);
                let frequency_hz = *frequency_hz;
                match key {
                    Key::Back => {
                        let selected = FREQUENCY_PRESETS
                            .iter()
                            .position(|(hz, _)| *hz == frequency_hz)
                            .unwrap_or(0);
                        self.screen = Screen::FrequencyChange { selected };
                    }
                    Key::Up => *cursor = (*cursor + row_count - 1) % row_count,
                    Key::Down => *cursor = (*cursor + 1) % row_count,
                    Key::Enter => *chosen ^= 1 << *cursor,
                    // SEND asks every ticked contact, then retunes here.
                    Key::Send => {
                        self.pending_frequency_requests = Some((frequency_hz, *chosen));
                        self.pending_frequency = Some(frequency_hz);
                        self.screen = Screen::FrequencyStatus {
                            status: FrequencyUiStatus::Queued,
                        };
                    }
                    _ => {}
                }
            }
            Screen::FrequencyStatus { .. } => {
                if key == Key::Back || key == Key::Enter {
                    self.screen = Screen::SettingsItem {
                        section: SettingsSection::Frequency,
                        item: 1,
                    };
                }
            }
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
                menu_open,
                selected_message,
                ..
            } if self.chat_input.is_empty() && !*menu_open => (
                *conversation,
                *unencrypted_dm,
                *from_discovery,
                *selected_message,
            ),
            _ => return false,
        };
        let Some(picked) = picked else { return false };
        let Some(row) = self.messages.get(picked) else {
            return false;
        };
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
}
