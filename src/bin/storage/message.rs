use alloc::vec::Vec;
use heapless::String;
use sequential_storage::map::{SerializationError, Value};

/// One bounded page of history, independent of the selected backend.
pub struct MessagePage {
    pub messages: Vec<StoredMessage>,
    pub next_offset: u32,
    pub done: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageStorageLocation {
    InternalFlash,
    SdCard,
}

impl<'a> Value<'a> for MessageStorageLocation {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        let value = match self {
            Self::InternalFlash => 0,
            Self::SdCard => 1,
        };
        let byte = buffer
            .first_mut()
            .ok_or(SerializationError::BufferTooSmall)?;
        *byte = value;
        Ok(1)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        match buffer.first() {
            Some(0) => Ok((Self::InternalFlash, 1)),
            Some(1) => Ok((Self::SdCard, 1)),
            Some(_) => Err(SerializationError::InvalidFormat),
            None => Err(SerializationError::BufferTooSmall),
        }
    }
}

/// Maximum UTF-8 payload bytes carried by one text/coordinate message.
pub const MAX_MESSAGE_PAYLOAD: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub enum MessageKind {
    Text,
    Coords { lat: f32, lon: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredMessage {
    pub id: u32,
    pub from_me: bool,
    pub timestamp: u32,
    pub kind: MessageKind,
    pub text: String<MAX_MESSAGE_PAYLOAD>,
    pub packet_id: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConversationKind {
    Broadcast,
    Dm([u8; 16]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub kind: ConversationKind,
    pub last_activity: u32,
    pub unread_count: u8,
    pub display_name: String<32>,
}

/// Legacy in-memory convenience type. Do not store whole conversation histories
/// as one flash map value; the StorageManager streams records to SD instead.
#[derive(Clone, Debug)]
pub struct MessageStore {
    pub messages: Vec<StoredMessage>,
}

const MESSAGE_FIXED_BYTES: usize = 1 + 4 + 4 + 2 + 1 + 1;

impl<'a> Value<'a> for StoredMessage {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        let coordinate_bytes = match &self.kind {
            MessageKind::Text => 0,
            MessageKind::Coords { .. } => 8,
        };
        let required = MESSAGE_FIXED_BYTES
            .checked_add(coordinate_bytes)
            .and_then(|n| n.checked_add(self.text.len()))
            .ok_or(SerializationError::InvalidData)?;
        if buffer.len() < required {
            return Err(SerializationError::BufferTooSmall);
        }

        let mut offset = 0;
        buffer[offset] = u8::from(self.from_me);
        offset += 1;
        buffer[offset..offset + 4].copy_from_slice(&self.id.to_le_bytes());
        offset += 4;
        buffer[offset..offset + 4].copy_from_slice(&self.timestamp.to_le_bytes());
        offset += 4;
        buffer[offset..offset + 2].copy_from_slice(&self.packet_id.to_le_bytes());
        offset += 2;

        match &self.kind {
            MessageKind::Text => {
                buffer[offset] = 0x01;
                offset += 1;
            }
            MessageKind::Coords { lat, lon } => {
                buffer[offset] = 0x02;
                offset += 1;
                buffer[offset..offset + 4].copy_from_slice(&lat.to_le_bytes());
                offset += 4;
                buffer[offset..offset + 4].copy_from_slice(&lon.to_le_bytes());
                offset += 4;
            }
        }

        buffer[offset] = self.text.len() as u8;
        offset += 1;
        buffer[offset..offset + self.text.len()].copy_from_slice(self.text.as_bytes());
        offset += self.text.len();
        Ok(offset)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        // Minimum header through the kind marker. Coordinate messages need 8 extra bytes.
        if buffer.len() < 12 {
            return Err(SerializationError::BufferTooSmall);
        }
        let mut offset = 0;
        let from_me = match buffer[offset] {
            0 => false,
            1 => true,
            _ => return Err(SerializationError::InvalidFormat),
        };
        offset += 1;

        let id = u32::from_le_bytes(
            buffer
                .get(offset..offset + 4)
                .ok_or(SerializationError::BufferTooSmall)?
                .try_into()
                .map_err(|_| SerializationError::InvalidFormat)?,
        );
        offset += 4;
        let timestamp = u32::from_le_bytes(
            buffer
                .get(offset..offset + 4)
                .ok_or(SerializationError::BufferTooSmall)?
                .try_into()
                .map_err(|_| SerializationError::InvalidFormat)?,
        );
        offset += 4;
        let packet_id = u16::from_le_bytes(
            buffer
                .get(offset..offset + 2)
                .ok_or(SerializationError::BufferTooSmall)?
                .try_into()
                .map_err(|_| SerializationError::InvalidFormat)?,
        );
        offset += 2;

        let kind_tag = *buffer
            .get(offset)
            .ok_or(SerializationError::BufferTooSmall)?;
        offset += 1;
        let kind = match kind_tag {
            0x01 => MessageKind::Text,
            0x02 => {
                let lat = f32::from_le_bytes(
                    buffer
                        .get(offset..offset + 4)
                        .ok_or(SerializationError::BufferTooSmall)?
                        .try_into()
                        .map_err(|_| SerializationError::InvalidFormat)?,
                );
                offset += 4;
                let lon = f32::from_le_bytes(
                    buffer
                        .get(offset..offset + 4)
                        .ok_or(SerializationError::BufferTooSmall)?
                        .try_into()
                        .map_err(|_| SerializationError::InvalidFormat)?,
                );
                offset += 4;
                MessageKind::Coords { lat, lon }
            }
            _ => return Err(SerializationError::InvalidFormat),
        };

        let text_len = *buffer
            .get(offset)
            .ok_or(SerializationError::BufferTooSmall)? as usize;
        offset += 1;
        if text_len > MAX_MESSAGE_PAYLOAD {
            return Err(SerializationError::InvalidData);
        }
        let text_end = offset
            .checked_add(text_len)
            .ok_or(SerializationError::InvalidData)?;
        let text_bytes = buffer
            .get(offset..text_end)
            .ok_or(SerializationError::BufferTooSmall)?;
        let text_str =
            core::str::from_utf8(text_bytes).map_err(|_| SerializationError::InvalidFormat)?;
        let mut text = String::<MAX_MESSAGE_PAYLOAD>::new();
        text.push_str(text_str)
            .map_err(|_| SerializationError::InvalidData)?;
        offset = text_end;

        Ok((
            Self {
                id,
                from_me,
                timestamp,
                kind,
                text,
                packet_id,
            },
            offset,
        ))
    }
}
