use alloc::vec::Vec;
use heapless::String;
use sequential_storage::map::{SerializationError, Value};

pub const MAX_MESSAGES_PER_CONVO: usize = 50;
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

#[derive(Clone, Debug)]
pub struct MessageStore {
    pub messages: Vec<StoredMessage>,
}

impl<'a> Value<'a> for StoredMessage {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        if buffer.len() < 1 + 4 + 1 + 4 + 1 + 2 + 1 + self.text.len() {
            return Err(SerializationError::BufferTooSmall);
        }
        let mut offset = 0;
        buffer[offset] = if self.from_me { 1 } else { 0 };
        offset += 1;
        buffer[offset..offset + 4].copy_from_slice(&self.id.to_le_bytes());
        offset += 4;
        buffer[offset..offset + 4].copy_from_slice(&self.timestamp.to_le_bytes());
        offset += 4;
        buffer[offset..offset + 2].copy_from_slice(&self.packet_id.to_le_bytes());
        offset += 2;
        match self.kind {
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
        if buffer.is_empty() {
            return Err(SerializationError::InvalidFormat);
        }
        let mut offset = 0;
        let from_me = buffer[offset] == 1;
        offset += 1;
        if buffer.len() < offset + 4 + 4 + 2 + 1 {
            return Err(SerializationError::BufferTooSmall);
        }
        let id = u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap());
        offset += 4;
        let timestamp = u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap());
        offset += 4;
        let packet_id = u16::from_le_bytes(buffer[offset..offset + 2].try_into().unwrap());
        offset += 2;
        let kind = match buffer[offset] {
            0x01 => {
                offset += 1;
                MessageKind::Text
            }
            0x02 => {
                offset += 1;
                if buffer.len() < offset + 8 {
                    return Err(SerializationError::BufferTooSmall);
                }
                let lat = f32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap());
                offset += 4;
                let lon = f32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap());
                offset += 4;
                MessageKind::Coords { lat, lon }
            }
            _ => return Err(SerializationError::InvalidFormat),
        };
        if buffer.len() < offset + 1 {
            return Err(SerializationError::BufferTooSmall);
        }
        let text_len = buffer[offset] as usize;
        offset += 1;
        if text_len > MAX_MESSAGE_PAYLOAD || buffer.len() < offset + text_len {
            return Err(SerializationError::BufferTooSmall);
        }
        let text_str = core::str::from_utf8(&buffer[offset..offset + text_len])
            .map_err(|_| SerializationError::InvalidFormat)?;
        let mut text = String::<MAX_MESSAGE_PAYLOAD>::new();
        text.push_str(text_str)
            .map_err(|_| SerializationError::InvalidData)?;
        offset += text_len;
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

#[derive(Clone, Debug)]
pub struct StoredMessageList(pub Vec<StoredMessage>);

impl<'a> Value<'a> for StoredMessageList {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        let mut total = 0;
        if buffer.len() < 4 {
            return Err(SerializationError::BufferTooSmall);
        }
        buffer[0..4].copy_from_slice(&(self.0.len() as u32).to_le_bytes());
        total += 4;
        for msg in &self.0 {
            let written = msg.serialize_into(&mut buffer[total..])?;
            total += written;
        }
        Ok(total)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        if buffer.len() < 4 {
            return Err(SerializationError::BufferTooSmall);
        }
        let count = u32::from_le_bytes(buffer[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        let mut messages = Vec::new();
        for _ in 0..count {
            let (msg, written) = StoredMessage::deserialize_from(&buffer[offset..])?;
            messages.push(msg);
            offset += written;
        }
        Ok((StoredMessageList(messages), offset))
    }
}
