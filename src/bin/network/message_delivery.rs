use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use heapless::String;

use crate::packet::packet_data::MAX_LOGICAL_MESSAGE_SIZE;

pub const MESSAGE_TEXT_CAPACITY: usize = MAX_LOGICAL_MESSAGE_SIZE;
pub const INCOMING_MESSAGE_CAPACITY: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IncomingMessage {
    Text {
        peer_id: [u8; 16],
        sender_id: [u8; 16],
        message_id: u16,
        text: String<MESSAGE_TEXT_CAPACITY>,
        encrypted: bool,
    },
    Coordinates {
        peer_id: [u8; 16],
        sender_id: [u8; 16],
        message_id: u16,
        latitude_microdegrees: i32,
        longitude_microdegrees: i32,
        encrypted: bool,
    },
    Acknowledgement {
        peer_id: [u8; 16],
        message_id: u16,
    },
}

pub type IncomingMessageQueue =
    Channel<CriticalSectionRawMutex, IncomingMessage, INCOMING_MESSAGE_CAPACITY>;
