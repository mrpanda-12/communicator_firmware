use sequential_storage::map::{Key, SerializationError};

const MAX_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageKey {
    bytes: [u8; MAX_KEY_BYTES],
    len: u8,
}

impl StorageKey {
    // Prefixes setting keys to separate them from contact keys.
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

    fn from_parts(namespace: u8, value: &[u8]) -> Self {
        let mut bytes = [0; MAX_KEY_BYTES];
        bytes[0] = namespace;
        bytes[1..value.len() + 1].copy_from_slice(value);

        Self {
            bytes,
            len: (value.len() + 1) as u8,
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
        // Validates the stored length before copying key bytes.
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
