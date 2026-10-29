use heapless::String;
use sequential_storage::map::{SerializationError, Value};

use crate::packet::packet_data::MAX_LOGICAL_MESSAGE_SIZE;

/// Storage key attributed to broadcast history.
pub const BROADCAST_ID: [u8; 16] = [0xff; 16];

/// One conversation: the public broadcast or a direct peer.
///
/// The UI, the receive path, and storage all use this one vocabulary, and the
/// storage key is derived from the peer ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversationId {
    Broadcast,
    Direct([u8; 16]),
}

impl ConversationId {
    /// Map a peer ID to its conversation; the all-zero ID means broadcast.
    pub fn from_peer(peer_id: [u8; 16]) -> Self {
        if peer_id == [0; 16] {
            Self::Broadcast
        } else {
            Self::Direct(peer_id)
        }
    }

    pub fn peer_id(self) -> [u8; 16] {
        match self {
            Self::Broadcast => [0; 16],
            Self::Direct(peer_id) => peer_id,
        }
    }

    pub fn is_broadcast(self) -> bool {
        matches!(self, Self::Broadcast)
    }

    /// Storage key of this conversation's history.
    pub fn storage_id(self) -> [u8; 16] {
        match self {
            Self::Broadcast => BROADCAST_ID,
            Self::Direct(peer_id) => peer_id,
        }
    }
}

/// Direction of one message relative to this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Sent,
    Received,
}

/// Body bytes one message can carry, matching the protocol limit.
pub const MAX_MESSAGE_PAYLOAD: usize = MAX_LOGICAL_MESSAGE_SIZE;

#[derive(Clone, Debug, PartialEq)]
pub enum MessageType {
    Text,
    Coords {
        latitude_microdegrees: i32,
        longitude_microdegrees: i32,
    },
}

impl MessageType {
    /// Returns the position this record carries, or `None` for plain text.
    ///
    /// Callers that build previews or rows use this instead of matching the
    /// enum themselves.
    pub fn coordinates(&self) -> Option<(i32, i32)> {
        match self {
            Self::Text => None,
            Self::Coords {
                latitude_microdegrees,
                longitude_microdegrees,
            } => Some((*latitude_microdegrees, *longitude_microdegrees)),
        }
    }
}

/// One chat message in its flash record form.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredMessage {
    /// Per-conversation record ordinal; display order comes from the record index.
    pub id: u32,
    pub from_me: bool,
    pub timestamp: u32,
    pub kind: MessageType,
    pub text: String<MAX_MESSAGE_PAYLOAD>,
    pub packet_id: u32,
    pub message_id: u32,
    pub sender_id: [u8; 16],
    pub encrypted: bool,
    /// Marks a sent message whose delivery the peer confirmed.
    pub acknowledged: bool,
}

impl StoredMessage {
    /// Build one record that is ready to append for a complete message.
    pub fn new(
        conversation_ordinal: u32,
        direction: Direction,
        timestamp: u32,
        kind: MessageType,
        text: &str,
        packet_id: u32,
        message_id: u32,
        sender_id: [u8; 16],
        encrypted: bool,
    ) -> Self {
        let mut stored_text = String::new();
        let _ = stored_text.push_str(text);
        Self {
            id: conversation_ordinal,
            from_me: direction == Direction::Sent,
            timestamp,
            kind,
            text: stored_text,
            packet_id,
            message_id,
            sender_id,
            encrypted,
            acknowledged: false,
        }
    }
}

// id, from_me, timestamp, packet_id, message_id, sender_id, encrypted, kind, text_len.
const MESSAGE_FIXED_BYTES: usize = 4 + 1 + 4 + 4 + 4 + 16 + 1 + 1 + 2;
/// One trailing byte records the acknowledged flag.
const ACK_FLAG_BYTES: usize = 1;

impl<'a> Value<'a> for StoredMessage {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        // Coordinates add eight bytes; text messages add only their text.
        let coordinate_bytes = match &self.kind {
            MessageType::Text => 0,
            MessageType::Coords { .. } => 8,
        };
        let required = MESSAGE_FIXED_BYTES
            .checked_add(coordinate_bytes)
            .and_then(|n| n.checked_add(self.text.len()))
            .and_then(|n| n.checked_add(ACK_FLAG_BYTES))
            .ok_or(SerializationError::InvalidData)?;
        if buffer.len() < required {
            return Err(SerializationError::BufferTooSmall);
        }

        let mut offset = 0;
        buffer[offset..offset + 4].copy_from_slice(&self.id.to_le_bytes());
        offset += 4;
        buffer[offset] = u8::from(self.from_me);
        offset += 1;
        buffer[offset..offset + 4].copy_from_slice(&self.timestamp.to_le_bytes());
        offset += 4;
        buffer[offset..offset + 4].copy_from_slice(&self.packet_id.to_le_bytes());
        offset += 4;
        buffer[offset..offset + 4].copy_from_slice(&self.message_id.to_le_bytes());
        offset += 4;
        buffer[offset..offset + 16].copy_from_slice(&self.sender_id);
        offset += 16;
        buffer[offset] = u8::from(self.encrypted);
        offset += 1;

        match &self.kind {
            MessageType::Text => {
                buffer[offset] = 0x01;
                offset += 1;
            }
            MessageType::Coords {
                latitude_microdegrees,
                longitude_microdegrees,
            } => {
                buffer[offset] = 0x02;
                offset += 1;
                buffer[offset..offset + 4].copy_from_slice(&latitude_microdegrees.to_le_bytes());
                offset += 4;
                buffer[offset..offset + 4].copy_from_slice(&longitude_microdegrees.to_le_bytes());
                offset += 4;
            }
        }

        buffer[offset..offset + 2].copy_from_slice(&(self.text.len() as u16).to_le_bytes());
        offset += 2;
        buffer[offset..offset + self.text.len()].copy_from_slice(self.text.as_bytes());
        offset += self.text.len();
        buffer[offset] = u8::from(self.acknowledged);
        offset += 1;
        Ok(offset)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        // The shortest record is a text message with no text.
        if buffer.len() < MESSAGE_FIXED_BYTES + ACK_FLAG_BYTES {
            return Err(SerializationError::BufferTooSmall);
        }
        let mut offset = 0;
        let id = u32::from_le_bytes(read_array::<4>(buffer, &mut offset)?);
        let from_me = match buffer[offset] {
            0 => false,
            1 => true,
            _ => return Err(SerializationError::InvalidFormat),
        };
        offset += 1;

        let timestamp = u32::from_le_bytes(read_array::<4>(buffer, &mut offset)?);
        let packet_id = u32::from_le_bytes(read_array::<4>(buffer, &mut offset)?);
        let message_id = u32::from_le_bytes(read_array::<4>(buffer, &mut offset)?);
        let sender_id = read_array::<16>(buffer, &mut offset)?;

        let encrypted = match buffer[offset] {
            0 => false,
            1 => true,
            _ => return Err(SerializationError::InvalidFormat),
        };
        offset += 1;

        let kind_tag = *buffer
            .get(offset)
            .ok_or(SerializationError::BufferTooSmall)?;
        offset += 1;
        let kind = match kind_tag {
            0x01 => MessageType::Text,
            0x02 => {
                let latitude_microdegrees =
                    i32::from_le_bytes(read_array::<4>(buffer, &mut offset)?);
                let longitude_microdegrees =
                    i32::from_le_bytes(read_array::<4>(buffer, &mut offset)?);
                MessageType::Coords {
                    latitude_microdegrees,
                    longitude_microdegrees,
                }
            }
            _ => return Err(SerializationError::InvalidFormat),
        };

        let text_len = u16::from_le_bytes(read_array::<2>(buffer, &mut offset)?) as usize;
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

        let acknowledged = match buffer.get(offset) {
            Some(0) | None => false,
            Some(1) => true,
            _ => return Err(SerializationError::InvalidFormat),
        };
        offset += 1;

        Ok((
            Self {
                id,
                from_me,
                timestamp,
                kind,
                text,
                packet_id,
                message_id,
                sender_id,
                encrypted,
                acknowledged,
            },
            offset,
        ))
    }
}

fn read_array<const N: usize>(
    buffer: &[u8],
    offset: &mut usize,
) -> Result<[u8; N], SerializationError> {
    let end = offset
        .checked_add(N)
        .ok_or(SerializationError::InvalidData)?;
    let bytes = buffer
        .get(*offset..end)
        .ok_or(SerializationError::BufferTooSmall)?;
    let array = bytes
        .try_into()
        .map_err(|_| SerializationError::InvalidFormat)?;
    *offset = end;
    Ok(array)
}
