use core::fmt::Write;

use heapless::{String, Vec};

const MAX_CONTACTS: usize = 32;
const CONTACT_NAME_CAPACITY: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversationKind {
    Broadcast,
    Direct([u8; 16]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationContact {
    pub kind: ConversationKind,
    pub custom_name: String<CONTACT_NAME_CAPACITY>,
    pub unread_count: u8,
}

impl ConversationContact {
    /// 1/ Keep the public broadcast conversation visible as the first contact.
    pub fn broadcast() -> Self {
        Self {
            kind: ConversationKind::Broadcast,
            custom_name: String::new(),
            unread_count: 0,
        }
    }

    pub fn direct(device_id: [u8; 16], name: &str) -> Self {
        let mut custom_name = String::new();
        let _ = custom_name.push_str(name);
        Self {
            kind: ConversationKind::Direct(device_id),
            custom_name,
            unread_count: 0,
        }
    }

    pub const fn is_deletable(&self) -> bool {
        !matches!(self.kind, ConversationKind::Broadcast)
    }

    /// 2/ Format the broadcast label, custom contact name, or short device ID.
    pub fn display_name(&self) -> String<CONTACT_NAME_CAPACITY> {
        let mut label = String::new();
        match self.kind {
            ConversationKind::Broadcast => {
                let _ = label.push_str("BROADCAST [PUBLIC]");
            }
            ConversationKind::Direct(_) if !self.custom_name.is_empty() => {
                let _ = label.push_str(self.custom_name.as_str());
            }
            ConversationKind::Direct(device_id) => {
                let _ = write!(label, "UNKNOWN {:02X}{:02X}...", device_id[0], device_id[1]);
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactBook {
    entries: Vec<ConversationContact, MAX_CONTACTS>,
}

impl ContactBook {
    /// 3/ Seed the contact list with the reserved broadcast conversation.
    pub fn new() -> Self {
        let mut entries = Vec::new();
        let _ = entries.push(ConversationContact::broadcast());
        Self { entries }
    }

    pub fn entries(&self) -> &[ConversationContact] {
        self.entries.as_slice()
    }

    pub fn get(&self, index: usize) -> Option<&ConversationContact> {
        self.entries.get(index)
    }

    pub fn add_direct(&mut self, device_id: [u8; 16], name: &str) -> Result<(), ContactBookError> {
        if device_id == [0; 16] {
            return Err(ContactBookError::ReservedBroadcastId);
        }
        if self
            .entries
            .iter()
            .any(|entry| entry.kind == ConversationKind::Direct(device_id))
        {
            return Ok(());
        }
        self.entries
            .push(ConversationContact::direct(device_id, name))
            .map_err(|_| ContactBookError::Full)
    }

    /// 4/ Refuse deletion of the broadcast entry while allowing direct contacts to be removed.
    pub fn remove(&mut self, index: usize) -> Result<ConversationContact, ContactBookError> {
        let Some(contact) = self.entries.get(index) else {
            return Err(ContactBookError::InvalidIndex);
        };
        if !contact.is_deletable() {
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
