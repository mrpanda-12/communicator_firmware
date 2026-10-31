//! A message the composer asks the run loop to send.

use heapless::Vec;

use crate::packet::packet_data::MAX_LOGICAL_MESSAGE_SIZE;
use crate::storage::message::{ConversationId, MessageType};

/// What the composer wants to send.
///
/// The text body carries no bytes: the composer buffer in `UiState` already
/// holds them, so a second copy here would reserve the whole 512-byte message
/// in every `UiState` even when the send is just coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutgoingBody {
    /// The open chat's composer text, read when the message goes out.
    Text,
    /// Coordinates from the latest GPS fix.
    Coords {
        latitude_microdegrees: i32,
        longitude_microdegrees: i32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutgoingMessage {
    pub conversation: ConversationId,
    pub unencrypted: bool,
    pub body: OutgoingBody,
}

impl OutgoingMessage {
    pub fn message_type(&self) -> MessageType {
        match &self.body {
            OutgoingBody::Text => MessageType::Text,
            OutgoingBody::Coords {
                latitude_microdegrees,
                longitude_microdegrees,
            } => MessageType::Coords {
                latitude_microdegrees: *latitude_microdegrees,
                longitude_microdegrees: *longitude_microdegrees,
            },
        }
    }

    /// Optional coordinate payload when this message carries a position.
    pub fn coordinates(&self) -> Option<(i32, i32)> {
        match &self.body {
            OutgoingBody::Text => None,
            OutgoingBody::Coords {
                latitude_microdegrees,
                longitude_microdegrees,
            } => Some((*latitude_microdegrees, *longitude_microdegrees)),
        }
    }

    /// True when this message sends the composer text.
    pub fn is_text(&self) -> bool {
        matches!(self.body, OutgoingBody::Text)
    }

    /// Sender-side packet payload without any packet framing. `text` is the
    /// composer buffer of the open chat and is ignored for coordinates.
    pub fn payload(&self, text: &str) -> Vec<u8, MAX_LOGICAL_MESSAGE_SIZE> {
        let mut payload = Vec::new();
        match &self.body {
            OutgoingBody::Text => {
                let _ = payload.extend_from_slice(text.as_bytes());
            }
            OutgoingBody::Coords {
                latitude_microdegrees,
                longitude_microdegrees,
            } => {
                let _ = payload.extend_from_slice(&latitude_microdegrees.to_be_bytes());
                let _ = payload.extend_from_slice(&longitude_microdegrees.to_be_bytes());
            }
        }
        payload
    }
}
