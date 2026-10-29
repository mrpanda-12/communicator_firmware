use sequential_storage::map::{Key, SerializationError};

const MAX_KEY_BYTES: usize = 32;

/// Keys carry a one-byte namespace so settings, contacts, and message records
/// cannot alias one another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageKey {
    bytes: [u8; MAX_KEY_BYTES],
    len: u8,
}

impl StorageKey {
    /// Setting names must be nonempty and at most 31 UTF-8 bytes.
    pub(crate) fn setting(name: &str) -> Option<Self> {
        let name = name.as_bytes();
        if name.is_empty() || name.len() + 1 > MAX_KEY_BYTES {
            return None;
        }
        Some(Self::from_parts(b's', name))
    }

    pub(crate) fn contact(device_id: &[u8; 16]) -> Self {
        Self::from_parts(b'c', device_id)
    }

    pub(crate) const fn is_contact(&self) -> bool {
        self.len == 17 && self.bytes[0] == b'c'
    }

    pub(crate) fn message(device_id: &[u8; 16], index: u32) -> Self {
        let mut value = [0; 20];
        value[..16].copy_from_slice(device_id);
        value[16..].copy_from_slice(&index.to_le_bytes());
        Self::from_parts(b'm', &value)
    }

    pub(crate) fn message_count(device_id: &[u8; 16]) -> Self {
        Self::from_parts(b'n', device_id)
    }

    pub(crate) fn message_base(device_id: &[u8; 16]) -> Self {
        Self::from_parts(b'b', device_id)
    }

    /// True for the record, count, and base namespaces of a conversation,
    /// whatever the device ID is, so "delete all chats" can enumerate them.
    pub(crate) fn is_conversation_message(&self) -> bool {
        matches!(self.bytes.first(), Some(b'm' | b'n' | b'b'))
    }

    fn from_parts(namespace: u8, value: &[u8]) -> Self {
        // Clamps the value to the 31-byte key budget; callers that accept
        // external text validate the length first.
        let length = value.len().min(MAX_KEY_BYTES - 1);
        let mut bytes = [0; MAX_KEY_BYTES];
        bytes[0] = namespace;
        bytes[1..length + 1].copy_from_slice(&value[..length]);
        Self {
            bytes,
            len: (length + 1) as u8,
        }
    }
}

impl Key for StorageKey {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        let len = self.len as usize;
        let encoded = buffer
            .get_mut(..len + 1)
            .ok_or(SerializationError::BufferTooSmall)?;
        encoded[0] = self.len;
        encoded[1..].copy_from_slice(&self.bytes[..len]);
        Ok(len + 1)
    }

    fn deserialize_from(buffer: &[u8]) -> Result<(Self, usize), SerializationError> {
        let len = *buffer.first().ok_or(SerializationError::BufferTooSmall)? as usize;
        if len == 0 || len > MAX_KEY_BYTES {
            return Err(SerializationError::InvalidData);
        }
        let encoded = buffer
            .get(1..len + 1)
            .ok_or(SerializationError::BufferTooSmall)?;
        let mut bytes = [0; MAX_KEY_BYTES];
        bytes[..len].copy_from_slice(encoded);
        Ok((
            Self {
                bytes,
                len: len as u8,
            },
            len + 1,
        ))
    }

    fn get_len(buffer: &[u8]) -> Result<usize, SerializationError> {
        let len = *buffer.first().ok_or(SerializationError::BufferTooSmall)? as usize;
        if len == 0 || len > MAX_KEY_BYTES {
            return Err(SerializationError::InvalidData);
        }
        if buffer.len() < len + 1 {
            return Err(SerializationError::BufferTooSmall);
        }
        Ok(len + 1)
    }
}
