use core::fmt::Write;

use heapless::{String, Vec};

pub use crate::storage::contact::Contact;
pub use crate::storage::message::ConversationId;

/// List capacity; the broadcast conversation occupies one slot.
pub const MAX_CONTACTS: usize = 32;
const CONTACT_NAME_CAPACITY: usize = 32;
/// Characters of the last message shown under a contact row.
pub const PREVIEW_CHARS: usize = 22;
/// Text capacity of a preview line, wide enough to never truncate a label.
const PREVIEW_BYTES: usize = 64;

/// One addressable contact: identity, optional pair key, chat preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactEntry {
    pub kind: ConversationId,
    pub custom_name: String<CONTACT_NAME_CAPACITY>,
    pub key: Option<[u8; 32]>,
    pub blocked: bool,
    pub preview: String<PREVIEW_BYTES>,
    /// Whether the preview line is our own last message.
    pub preview_from_me: bool,
    /// Whether that last message of ours was acknowledged.
    pub preview_acknowledged: bool,
    /// UTC seconds of the newest record; 0 when no GPS date was known.
    pub last_activity: u32,
}

impl ContactEntry {
    /// 1/ Keep the public broadcast conversation visible as the first entry.
    pub fn broadcast() -> Self {
        Self {
            kind: ConversationId::Broadcast,
            custom_name: String::new(),
            key: None,
            blocked: false,
            preview: String::new(),
            preview_from_me: false,
            preview_acknowledged: false,
            last_activity: 0,
        }
    }

    pub fn paired(device_id: [u8; 16], name: &str, key: [u8; 32], blocked: bool) -> Self {
        let mut custom_name = String::new();
        let _ = custom_name.push_str(name);
        Self {
            kind: ConversationId::Direct(device_id),
            custom_name,
            key: Some(key),
            blocked,
            preview: String::new(),
            preview_from_me: false,
            preview_acknowledged: false,
            last_activity: 0,
        }
    }

    /// A peer heard on the air but never paired or saved as a contact.
    /// It gets a plain list row until the user blocks or deletes it.
    pub fn unknown(device_id: [u8; 16]) -> Self {
        Self {
            kind: ConversationId::Direct(device_id),
            custom_name: String::new(),
            key: None,
            blocked: false,
            preview: String::new(),
            preview_from_me: false,
            preview_acknowledged: false,
            last_activity: 0,
        }
    }

    pub fn is_deletable(&self) -> bool {
        !matches!(self.kind, ConversationId::Broadcast)
    }

    /// True when this entry describes an air-heard peer that was never paired.
    pub fn is_unknown(&self) -> bool {
        matches!(self.kind, ConversationId::Direct(_)) && self.key.is_none()
    }

    /// 2/ Format the broadcast label, custom contact name, or short device ID.
    pub fn display_name(&self) -> String<CONTACT_NAME_CAPACITY> {
        let mut label = String::new();
        match self.kind {
            ConversationId::Broadcast => {
                let _ = label.push_str("BROADCAST [PUBLIC]");
            }
            ConversationId::Direct(_) if !self.custom_name.is_empty() => {
                let _ = label.push_str(self.custom_name.as_str());
            }
            ConversationId::Direct(device_id) => {
                let security = if self.key.is_some() {
                    "PAIRED"
                } else {
                    "UNKNOWN"
                };
                let _ = write!(
                    label,
                    "{security} {:02X}{:02X}...",
                    device_id[0], device_id[1]
                );
            }
        }
        label
    }

    /// 3/ Name for a toast: the custom name, else the full device ID.
    pub fn name_or_id(&self) -> String<CONTACT_NAME_CAPACITY> {
        let mut label = String::new();
        match self.kind {
            ConversationId::Broadcast => {
                let _ = label.push_str("BROADCAST [PUBLIC]");
            }
            ConversationId::Direct(device_id) if self.custom_name.is_empty() => {
                label = hex_id(device_id);
            }
            ConversationId::Direct(_) => {
                let _ = label.push_str(self.custom_name.as_str());
            }
        }
        label
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactBookError {
    Full,
    BroadcastCannotBeDeleted,
    ReservedBroadcastId,
    InvalidIndex,
}

/// The live address book. Flash keeps one `Contact` per paired device; the book
/// holds previews too, so the chat list renders without touching flash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactBook {
    entries: Vec<ContactEntry, MAX_CONTACTS>,
}

impl ContactBook {
    /// 3/ Seed the contact list with the reserved broadcast conversation.
    pub fn new() -> Self {
        let mut entries = Vec::new();
        let _ = entries.push(ContactEntry::broadcast());
        Self { entries }
    }

    pub fn entries(&self) -> &[ContactEntry] {
        self.entries.as_slice()
    }

    pub fn entry(&self, index: usize) -> Option<&ContactEntry> {
        self.entries.get(index)
    }

    pub fn entry_mut(&mut self, index: usize) -> Option<&mut ContactEntry> {
        self.entries.get_mut(index)
    }

    /// Visible list rows of this book, skipping blocked entries.
    ///
    /// Returns the *storage indices* in display order; renderer and key
    /// handling always translate through this helper.
    pub fn visible_indices(&self, skip_broadcast: bool) -> Vec<usize, MAX_CONTACTS> {
        let mut indices = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.blocked {
                continue;
            }
            if skip_broadcast && index == 0 {
                continue;
            }
            let _ = indices.push(index);
        }
        indices
    }

    pub fn entry_by_kind(&self, kind: ConversationId) -> Option<&ContactEntry> {
        self.entries.iter().find(|entry| entry.kind == kind)
    }

    pub fn contains_direct(&self, device_id: &[u8; 16]) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.kind == ConversationId::Direct(*device_id))
    }

    pub fn direct_index(&self, device_id: &[u8; 16]) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.kind == ConversationId::Direct(*device_id))
    }

    pub fn direct_key(&self, device_id: &[u8; 16]) -> Option<&[u8; 32]> {
        self.entries
            .iter()
            .find(|entry| entry.kind == ConversationId::Direct(*device_id))?
            .key
            .as_ref()
    }

    /// Insert or overwrite a direct entry from a persisted contact record.
    pub fn adopt(&mut self, contact: &Contact) -> Result<(), ContactBookError> {
        if contact.device_id == [0; 16] {
            return Err(ContactBookError::ReservedBroadcastId);
        }
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|entry| entry.kind == ConversationId::Direct(contact.device_id))
        {
            existing.custom_name = contact.custom_name.clone();
            existing.key = Some(contact.key);
            existing.blocked = contact.blocked;
            return Ok(());
        }
        self.entries
            .push(ContactEntry::paired(
                contact.device_id,
                contact.custom_name.as_str(),
                contact.key,
                contact.blocked,
            ))
            .map_err(|_| ContactBookError::Full)
    }

    /// Replace one entry's label; an empty name falls back to the device ID.
    pub fn set_name(&mut self, device_id: [u8; 16], name: &str) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.kind == ConversationId::Direct(device_id))
        {
            entry.custom_name.clear();
            let _ = entry.custom_name.push_str(name);
        }
    }

    /// Register an unpaired sender so it shows up in the chats list.
    /// Returns `false` when an entry for this device already exists.
    pub fn note_unknown(&mut self, device_id: [u8; 16]) -> bool {
        if self.contains_direct(&device_id) {
            return false;
        }
        self.entries.push(ContactEntry::unknown(device_id)).is_ok()
    }

    /// Seed one chat-list preview from flash at boot or after an event.
    pub fn seed_preview(
        &mut self,
        kind: ConversationId,
        preview: &str,
        from_me: bool,
        acknowledged: bool,
        timestamp: u32,
    ) {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.kind == kind) else {
            return;
        };
        entry.preview.clear();
        let _ = entry.preview.push_str(preview);
        entry.preview_from_me = from_me;
        entry.preview_acknowledged = acknowledged;
        entry.last_activity = timestamp;
    }

    /// Chat row order: the conversation with the newest record first. Equal
    /// timestamps keep the book's order, so rows only move on a new message.
    pub fn chat_indices(&self) -> Vec<usize, MAX_CONTACTS> {
        let mut order = self.visible_indices(false);
        for position in 1..order.len() {
            let mut index = position;
            while index > 0 {
                let previous = self.entries[order[index - 1]].last_activity;
                let current = self.entries[order[index]].last_activity;
                if previous >= current {
                    break;
                }
                order.swap(index - 1, index);
                index -= 1;
            }
        }
        order
    }

    /// 4/ A blocked peer is hidden everywhere until it is unblocked again.
    pub fn set_blocked(&mut self, device_id: &[u8; 16], blocked: bool) -> bool {
        self.entries
            .iter_mut()
            .find(|entry| entry.kind == ConversationId::Direct(*device_id))
            .is_some_and(|entry| {
                entry.blocked = blocked;
                true
            })
    }

    /// 5/ Delete a direct contact entry; broadcast stays.
    pub fn remove(&mut self, index: usize) -> Result<ContactEntry, ContactBookError> {
        let Some(entry) = self.entries.get(index) else {
            return Err(ContactBookError::InvalidIndex);
        };
        if !entry.is_deletable() {
            return Err(ContactBookError::BroadcastCannotBeDeleted);
        }
        Ok(self.entries.remove(index))
    }
}

impl Default for ContactBook {
    fn default() -> Self {
        Self::new()
    }
}

/// Full uppercase hex ID, used when a peer has no saved name.
pub fn hex_id(device_id: [u8; 16]) -> String<CONTACT_NAME_CAPACITY> {
    let mut label = String::new();
    for byte in device_id {
        let _ = write!(label, "{byte:02X}");
    }
    label
}

/// Cut one preview line for a message body.
pub fn preview_text(text: &str, coordinates: Option<(i32, i32)>) -> String<PREVIEW_BYTES> {
    let mut preview = String::new();
    match coordinates {
        Some(_) => {
            let _ = preview.push_str("[GPS]");
        }
        None => {
            // A cut line ends in `..`, so the row shows text was dropped.
            let total = text.chars().count();
            let visible = if total > PREVIEW_CHARS {
                PREVIEW_CHARS - 2
            } else {
                PREVIEW_CHARS
            };
            for character in text.chars().take(visible) {
                if preview.push(character).is_err() {
                    break;
                }
            }
            if total > PREVIEW_CHARS {
                let _ = preview.push_str("..");
            }
        }
    }
    preview
}
