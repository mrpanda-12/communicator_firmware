use core::fmt::Write;

use heapless::{String, Vec};

pub use crate::storage::contact::Contact;
pub use crate::storage::message::ConversationId;

const MAX_CONTACTS: usize = 32;
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
    ) {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.kind == kind) else {
            return;
        };
        entry.preview.clear();
        let _ = entry.preview.push_str(preview);
        entry.preview_from_me = from_me;
        entry.preview_acknowledged = acknowledged;
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

/// Cut one preview line for a message body.
pub fn preview_text(text: &str, coordinates: Option<(i32, i32)>) -> String<PREVIEW_BYTES> {
    let mut preview = String::new();
    match coordinates {
        Some(_) => {
            let _ = preview.push_str("[GPS]");
        }
        None => {
            for character in text.chars().take(PREVIEW_CHARS) {
                if preview.push(character).is_err() {
                    break;
                }
            }
        }
    }
    preview
}
