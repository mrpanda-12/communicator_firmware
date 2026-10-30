//! Tests for the firmware's pure logic.

use heapless::{String, Vec};
use sequential_storage::map::{Key, Value};

use crate::calendar::{civil_from_days, days_from_civil};
use crate::crypto::KEY_SIZE;
use crate::hex::{DEVICE_ID_CHARS, hex_device_id, write_hex};
use crate::network::outbox;
use crate::notify::{self, PopupType};
use crate::packet::fragment::{Defragmenter, Fragmenter};
use crate::packet::packet_data::{
    AUTHENTICATED_HEADER_SIZE, BROADCAST_RECIPIENT, DEFAULT_HOP_LIMIT, GCM_OVERHEAD, HEADER_SIZE,
    MAX_FRAGMENT_COUNT, MAX_LOGICAL_MESSAGE_SIZE, MAX_PACKET_SIZE, MAX_PAYLOAD_SIZE, PacketData,
    PROTOCOL_VERSION, last_message_id, next_packet_id, set_packet_ids,
};
use crate::packet::packet_deserializer::PacketDeserializer;
use crate::packet::packet_error::PacketError;
use crate::packet::packet_serializer::PacketSerializer;
use crate::packet::packet_type::PacketType;
use crate::radio::types::{
    CodingRate, ConfigError, FrequencyBand, LoraBandwidth, LrFhssProfile, PaMode, RadioConfig,
};
use crate::storage::contact::Contact;
use crate::storage::message::{
    BROADCAST_STORAGE_ID, ConversationId, MAX_MESSAGE_PAYLOAD, MessageType, StoredMessage,
};
use crate::storage::settings::StorageKey;
use crate::ui::contact::{ContactBook, ContactEntry, preview_text};
use crate::ui::navigation::{compass_direction, distance_and_bearing, relative_turn_degrees};

fn id(byte: u8) -> [u8; 16] {
    [byte; 16]
}

fn hstring<const N: usize>(text: &str) -> String<N> {
    let mut value = String::new();
    value.push_str(text).unwrap();
    value
}

fn packet(payload: &[u8]) -> PacketData {
    PacketData::new(
        0x0102_0304,
        0x0506_0708,
        0,
        1,
        id(0xAA),
        id(0xBB),
        false,
        PacketType::Text,
        payload,
        DEFAULT_HOP_LIMIT,
        DEFAULT_HOP_LIMIT,
    )
    .unwrap()
}

fn assert_same_packet(left: &PacketData, right: &PacketData) {
    assert_eq!(left.version, right.version);
    assert_eq!(left.packet_id, right.packet_id);
    assert_eq!(left.message_id, right.message_id);
    assert_eq!(left.fragment_index, right.fragment_index);
    assert_eq!(left.fragment_count, right.fragment_count);
    assert_eq!(left.from, right.from);
    assert_eq!(left.to, right.to);
    assert_eq!(left.is_encrypted, right.is_encrypted);
    assert_eq!(left.packet_type, right.packet_type);
    assert_eq!(left.payload.as_slice(), right.payload.as_slice());
    assert_eq!(left.initial_hop_limit, right.initial_hop_limit);
    assert_eq!(left.hop_limit, right.hop_limit);
}

fn stored_text() -> StoredMessage {
    StoredMessage::new(
        7,
        true,
        1_234,
        MessageType::Text,
        "hello",
        0x0102,
        0x0102,
        id(0x33),
        true,
    )
}

fn contact(device_byte: u8, name: &str, key: u8, blocked: bool) -> Contact {
    Contact {
        device_id: id(device_byte),
        key: [key; KEY_SIZE],
        custom_name: hstring(name),
        blocked,
    }
}

mod calendar;
mod hex;
mod packet_framing;
mod codec;
mod fragmentation;
mod storage;
mod radio;
mod navigation;
mod contacts;
mod notifications;
mod outbox_retry;
mod crypto;
