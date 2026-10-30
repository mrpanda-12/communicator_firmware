//! Host-side tests for the firmware's pure logic.

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

// ---- calendar -----------------------------------------------------------

#[test]
fn civil_epoch_is_day_zero() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(civil_from_days(0), (1970, 1, 1));
}

#[test]
fn civil_round_trips_known_dates() {
    for (year, month, day) in [
        (2000i32, 1u32, 1u32),
        (2000, 12, 31),
        (2026, 10, 28),
        (2100, 3, 1),
        (1999, 12, 31),
    ] {
        let days = days_from_civil(year, month, day);
        assert_eq!(
            civil_from_days(days),
            (year, month as i32, day as i32),
            "round trip failed for {year}-{month}-{day}"
        );
    }
}

#[test]
fn civil_handles_leap_day() {
    let leap = days_from_civil(2000, 2, 29);
    assert_eq!(civil_from_days(leap), (2000, 2, 29));
    assert_eq!(days_from_civil(2000, 3, 1), leap + 1);
    // 1900 was not a leap year: the algorithm must skip Feb 29.
    assert_eq!(days_from_civil(1900, 3, 1), days_from_civil(1900, 2, 28) + 1);
}

#[test]
fn civil_negative_days_go_before_epoch() {
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
    assert_eq!(days_from_civil(1969, 12, 31), -1);
}

#[test]
fn civil_clamps_month_overflow() {
    // The clock only ever passes 1..=12; the clamp is documented behaviour.
    assert_eq!(days_from_civil(2026, 13, 1), days_from_civil(2026, 12, 1));
}

#[test]
fn epoch_2000_offset_matches_storage_format() {
    // Stored timestamps count seconds since 2000-01-01.
    assert_eq!(days_from_civil(2000, 1, 1), 10_957);
}

// ---- hex ----------------------------------------------------------------

#[test]
fn hex_device_id_formats_all_bytes() {
    let mut device = [0u8; 16];
    device[0] = 0x00;
    device[1] = 0x0F;
    device[15] = 0xFF;
    let formatted = hex_device_id(&device);
    assert_eq!(formatted.len(), DEVICE_ID_CHARS);
    assert!(formatted.starts_with("000F"));
    assert!(formatted.ends_with("FF"));
}

#[test]
fn write_hex_supports_separators() {
    let mut plain = String::<8>::new();
    write_hex(&mut plain, &[0x0A, 0xFF], None).unwrap();
    assert_eq!(plain.as_str(), "0AFF");

    let mut separated = String::<8>::new();
    write_hex(&mut separated, &[0x0A, 0xFF], Some(':')).unwrap();
    assert_eq!(separated.as_str(), "0A:FF");
}

#[test]
fn write_hex_empty_and_capacity() {
    let mut empty = String::<4>::new();
    write_hex(&mut empty, &[], None).unwrap();
    assert_eq!(empty.as_str(), "");

    let mut too_small = String::<3>::new();
    assert!(write_hex(&mut too_small, &[0xAA, 0xBB], None).is_err());
}

// ---- packet framing -----------------------------------------------------

#[test]
fn protocol_constants_match_the_specification() {
    assert_eq!(PROTOCOL_VERSION, 2);
    assert_eq!(HEADER_SIZE, 47);
    assert_eq!(AUTHENTICATED_HEADER_SIZE, HEADER_SIZE - 1);
    assert_eq!(MAX_PACKET_SIZE, 255);
    assert_eq!(MAX_PAYLOAD_SIZE, MAX_PACKET_SIZE - HEADER_SIZE);
    assert_eq!(MAX_LOGICAL_MESSAGE_SIZE, 512);
    assert_eq!(MAX_FRAGMENT_COUNT, 16);
    assert_eq!(GCM_OVERHEAD, 28);
    assert_eq!(DEFAULT_HOP_LIMIT, 3);
}

#[test]
fn header_layout_is_stable() {
    let packet = packet(b"hi");
    let mut header = [0u8; HEADER_SIZE];
    packet.write_header(&mut header);

    assert_eq!(header[0], PROTOCOL_VERSION);
    assert_eq!(&header[1..5], &0x0102_0304u32.to_be_bytes());
    assert_eq!(&header[5..9], &0x0506_0708u32.to_be_bytes());
    assert_eq!(header[9], 0);
    assert_eq!(header[10], 1);
    assert_eq!(&header[11..27], &id(0xAA));
    assert_eq!(&header[27..43], &id(0xBB));
    assert_eq!(header[43], 0);
    assert_eq!(header[44], PacketType::Text as u8);
    assert_eq!(header[45], DEFAULT_HOP_LIMIT);
    assert_eq!(header[46], DEFAULT_HOP_LIMIT);
}

#[test]
fn authenticated_header_excludes_hop_limit() {
    let packet = packet(b"hi");
    let mut header = [0u8; HEADER_SIZE];
    packet.write_header(&mut header);
    assert_eq!(
        packet.authenticated_header().as_slice(),
        &header[..AUTHENTICATED_HEADER_SIZE]
    );
}

#[test]
fn packet_validate_accepts_valid_packet() {
    assert_eq!(packet(b"payload").validate(), Ok(()));
}

#[test]
fn packet_rejects_bad_fragment_metadata() {
    let zero_count = PacketData::new(
        1,
        1,
        0,
        0,
        id(1),
        id(2),
        false,
        PacketType::Text,
        b"x",
        3,
        3,
    );
    assert_eq!(
        zero_count.err().unwrap(),
        PacketError::InvalidFragmentMetadata
    );

    let index_out_of_range = PacketData::new(
        1,
        1,
        2,
        2,
        id(1),
        id(2),
        false,
        PacketType::Text,
        b"x",
        3,
        3,
    );
    assert_eq!(
        index_out_of_range.err().unwrap(),
        PacketError::InvalidFragmentMetadata
    );

    let mut too_many = packet(b"x");
    too_many.fragment_count = (MAX_FRAGMENT_COUNT + 1) as u8;
    assert_eq!(
        too_many.validate().unwrap_err(),
        PacketError::InvalidFragmentMetadata
    );
}

#[test]
fn packet_rejects_oversized_payload() {
    let oversized = [0u8; MAX_PAYLOAD_SIZE + 1];
    assert_eq!(
        PacketData::new(
            1,
            1,
            0,
            1,
            id(1),
            id(2),
            false,
            PacketType::Text,
            &oversized,
            3,
            3
        )
        .err()
        .unwrap(),
        PacketError::PayloadTooLarge
    );
}

#[test]
fn packet_rejects_short_encrypted_payload() {
    assert_eq!(
        PacketData::new(
            1,
            1,
            0,
            1,
            id(1),
            id(2),
            true,
            PacketType::Text,
            &[0; GCM_OVERHEAD - 1],
            3,
            3
        )
        .err()
        .unwrap(),
        PacketError::InvalidEncryptedPayload
    );
}

#[test]
fn packet_rejects_hop_limit_above_initial() {
    let mut packet = packet(b"x");
    packet.hop_limit = packet.initial_hop_limit + 1;
    assert_eq!(packet.validate().unwrap_err(), PacketError::InvalidHopLimit);
}

#[test]
fn packet_broadcast_and_addressing() {
    let broadcast = PacketData::new(
        1,
        1,
        0,
        1,
        id(1),
        BROADCAST_RECIPIENT,
        false,
        PacketType::DeviceAnnounce,
        b"",
        3,
        3,
    )
    .unwrap();
    assert!(broadcast.is_broadcast());
    assert!(broadcast.is_for(&id(9)));

    let direct = packet(b"x");
    assert!(!direct.is_broadcast());
    assert!(direct.is_for(&id(0xBB)));
    assert!(!direct.is_for(&id(0xCC)));
}

#[test]
fn packet_hop_limit_decrements_to_zero() {
    let mut packet = packet(b"x");
    assert!(packet.can_forward());
    for expected in [2u8, 1, 0] {
        assert!(packet.decrement_hop_limit());
        assert_eq!(packet.hop_limit, expected);
    }
    assert!(!packet.can_forward());
    assert!(!packet.decrement_hop_limit());
}

#[test]
fn packet_application_payload_len_accounts_for_gcm() {
    assert_eq!(packet(b"hello").application_payload_len(), Some(5));

    let encrypted = PacketData::new(
        1,
        1,
        0,
        1,
        id(1),
        id(2),
        true,
        PacketType::Text,
        &[0; GCM_OVERHEAD + 4],
        3,
        3,
    )
    .unwrap();
    assert_eq!(encrypted.application_payload_len(), Some(4));

    let too_short = packet(&[0; 3]);
    assert_eq!(too_short.application_payload_len(), Some(3));
}

#[test]
fn packet_type_round_trips_known_values() {
    for (byte, kind) in [
        (0x01, PacketType::Text),
        (0x02, PacketType::Ack),
        (0x03, PacketType::Coordinates),
        (0x04, PacketType::Heading),
        (0x05, PacketType::DeviceAnnounce),
        (0x06, PacketType::FrequencyChange),
        (0x07, PacketType::DeviceAnnounceResponse),
        (0x08, PacketType::PairingConfirm),
    ] {
        assert_eq!(PacketType::try_from(byte), Ok(kind));
        assert_eq!(kind as u8, byte);
    }
    assert!(PacketType::try_from(0x00).is_err());
    assert!(PacketType::try_from(0x09).is_err());
}

#[test]
fn packet_id_counters_are_settable() {
    set_packet_ids(41, 40);
    assert_eq!(next_packet_id(), 41);
    assert_eq!(last_message_id(), 40);
    set_packet_ids(0, 0);
    assert_eq!(next_packet_id(), 0);
    assert_eq!(last_message_id(), 0);
}

// ---- serialization ------------------------------------------------------

#[test]
fn serialize_deserialize_round_trip() {
    let original = packet(b"hello wire");
    let mut buffer = [0u8; MAX_PACKET_SIZE];
    let len = PacketSerializer::serialize(&original, &mut buffer).unwrap();
    assert_eq!(len, original.encoded_len());

    let decoded = PacketDeserializer::deserialize(&buffer[..len]).unwrap();
    assert_same_packet(&original, &decoded);
}

#[test]
fn serialize_rejects_small_buffer() {
    let original = packet(b"hello");
    let mut buffer = [0u8; HEADER_SIZE];
    assert_eq!(
        PacketSerializer::serialize(&original, &mut buffer).unwrap_err(),
        PacketError::BufferTooSmall
    );
}

#[test]
fn deserialize_rejects_short_frame() {
    let frame = [0u8; HEADER_SIZE - 1];
    assert_eq!(
        PacketDeserializer::deserialize(&frame).err().unwrap(),
        PacketError::PacketTooShort
    );
}

#[test]
fn deserialize_rejects_bad_version() {
    let mut frame = [0u8; HEADER_SIZE];
    frame[0] = PROTOCOL_VERSION + 1;
    assert_eq!(
        PacketDeserializer::deserialize(&frame).err().unwrap(),
        PacketError::UnsupportedVersion
    );
}

#[test]
fn deserialize_rejects_bad_encryption_flag() {
    let mut frame = [0u8; HEADER_SIZE];
    frame[0] = PROTOCOL_VERSION;
    frame[9] = 0;
    frame[10] = 1;
    frame[43] = 2;
    assert_eq!(
        PacketDeserializer::deserialize(&frame).err().unwrap(),
        PacketError::InvalidEncryptionFlag
    );
}

#[test]
fn deserialize_rejects_unknown_packet_type() {
    let mut frame = [0u8; HEADER_SIZE];
    frame[0] = PROTOCOL_VERSION;
    frame[9] = 0;
    frame[10] = 1;
    frame[43] = 0;
    frame[44] = 0x7F;
    assert_eq!(
        PacketDeserializer::deserialize(&frame).err().unwrap(),
        PacketError::InvalidPacketType
    );
}

// ---- fragmentation ------------------------------------------------------

#[test]
fn fragment_count_matches_plaintext_capacity() {
    let limit = MAX_PACKET_SIZE;
    assert_eq!(Fragmenter::fragment_count_with_limit(false, 0, limit), Ok(1));
    assert_eq!(Fragmenter::fragment_count_with_limit(false, 1, limit), Ok(1));
    assert_eq!(
        Fragmenter::fragment_count_with_limit(false, MAX_PAYLOAD_SIZE, limit),
        Ok(1)
    );
    assert_eq!(
        Fragmenter::fragment_count_with_limit(false, MAX_PAYLOAD_SIZE + 1, limit),
        Ok(2)
    );
    assert_eq!(
        Fragmenter::fragment_count_with_limit(false, MAX_LOGICAL_MESSAGE_SIZE, limit),
        Ok(3)
    );
}

#[test]
fn fragment_count_accounts_for_encryption_overhead() {
    let limit = MAX_PACKET_SIZE;
    let capacity = MAX_PAYLOAD_SIZE - GCM_OVERHEAD;
    assert_eq!(
        Fragmenter::fragment_count_with_limit(true, capacity, limit),
        Ok(1)
    );
    assert_eq!(
        Fragmenter::fragment_count_with_limit(true, capacity + 1, limit),
        Ok(2)
    );
}

#[test]
fn fragment_count_uses_the_band_packet_limit() {
    // S-band allows 133-byte frames.
    let limit = 133;
    let capacity = limit - HEADER_SIZE;
    assert_eq!(
        Fragmenter::fragment_count_with_limit(false, capacity, limit),
        Ok(1)
    );
    assert_eq!(
        Fragmenter::fragment_count_with_limit(false, capacity + 1, limit),
        Ok(2)
    );
    assert!(Fragmenter::fragment_count_with_limit(false, 512, limit).is_ok());
}

#[test]
fn fragment_count_rejects_impossible_limits() {
    assert_eq!(
        Fragmenter::fragment_count_with_limit(false, 1, HEADER_SIZE - 1).unwrap_err(),
        PacketError::PayloadTooLarge
    );
    assert_eq!(
        Fragmenter::fragment_count_with_limit(false, 1, MAX_PACKET_SIZE + 1).unwrap_err(),
        PacketError::PayloadTooLarge
    );
    assert_eq!(
        Fragmenter::fragment_count_with_limit(false, MAX_LOGICAL_MESSAGE_SIZE + 1, MAX_PACKET_SIZE)
            .unwrap_err(),
        PacketError::PayloadTooLarge
    );
    // No room for the GCM nonce and tag.
    assert_eq!(
        Fragmenter::fragment_count_with_limit(true, 1, HEADER_SIZE + GCM_OVERHEAD).unwrap_err(),
        PacketError::PayloadTooLarge
    );
}

#[test]
fn fragment_ids_are_consecutive() {
    let payload = [0x5A; MAX_LOGICAL_MESSAGE_SIZE];
    let packets = Fragmenter::fragment_with_defaults_and_limit(
        100,
        100,
        id(1),
        id(2),
        false,
        PacketType::Text,
        &payload,
        MAX_PACKET_SIZE,
    )
    .unwrap();

    assert_eq!(packets.len(), 3);
    for (index, packet) in packets.iter().enumerate() {
        assert_eq!(packet.packet_id, 100 + index as u32);
        assert_eq!(packet.message_id, 100);
        assert_eq!(packet.fragment_index, index as u8);
        assert_eq!(packet.fragment_count, 3);
        assert_eq!(packet.initial_hop_limit, DEFAULT_HOP_LIMIT);
        assert_eq!(packet.hop_limit, DEFAULT_HOP_LIMIT);
        assert!(!packet.is_encrypted);
    }
    // Every fragment but the last is full.
    assert_eq!(packets[0].payload.len(), MAX_PAYLOAD_SIZE);
    assert_eq!(packets[2].payload.len(), MAX_LOGICAL_MESSAGE_SIZE - 2 * MAX_PAYLOAD_SIZE);
}

#[test]
fn defragmenter_reassembles_out_of_order() {
    let mut payload = [0u8; MAX_LOGICAL_MESSAGE_SIZE];
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte = index as u8;
    }
    let packets = Fragmenter::fragment_with_defaults_and_limit(
        500,
        500,
        id(1),
        id(2),
        false,
        PacketType::Text,
        &payload,
        MAX_PACKET_SIZE,
    )
    .unwrap();

    let mut defragmenter = Defragmenter::new();
    let mut result = None;
    for packet in packets.iter().rev() {
        let plaintext = packet.payload.as_slice().to_vec();
        result = defragmenter
            .push(packet.clone(), &plaintext, [0x77; 32])
            .unwrap();
    }

    let message = result.expect("complete message expected");
    assert_eq!(message.payload.as_slice(), &payload);
    assert!(!message.encrypted);
    assert_eq!(message.first_packet.fragment_index, 0);
    assert_eq!(message.accepted_fragments.len(), packets.len());
}

#[test]
fn defragmenter_ignores_exact_duplicate() {
    let payload = [1u8; MAX_PAYLOAD_SIZE + 1];
    let packets = Fragmenter::fragment_with_defaults_and_limit(
        600,
        600,
        id(1),
        id(2),
        false,
        PacketType::Text,
        &payload,
        MAX_PACKET_SIZE,
    )
    .unwrap();

    let mut defragmenter = Defragmenter::new();
    let first = packets[0].payload.as_slice().to_vec();
    assert!(defragmenter
        .push(packets[0].clone(), &first, [1; 32])
        .unwrap()
        .is_none());

    let duplicate = defragmenter.push(packets[0].clone(), &first, [1; 32]).unwrap();
    assert!(duplicate.is_none());

    // Same index, different packet ID: the assembly is rejected.
    let mut conflicting = packets[0].clone();
    conflicting.packet_id += 1;
    assert_eq!(
        defragmenter                .push(conflicting, &first, [2; 32])
            .err()
            .unwrap(),
        PacketError::InvalidFragmentMetadata
    );
}

#[test]
fn defragmenter_rejects_inconsistent_fragment_count() {
    let payload = [7u8; MAX_PAYLOAD_SIZE + 1];
    let packets = Fragmenter::fragment_with_defaults_and_limit(
        700,
        700,
        id(1),
        id(2),
        false,
        PacketType::Text,
        &payload,
        MAX_PACKET_SIZE,
    )
    .unwrap();

    let mut defragmenter = Defragmenter::new();
    let first = packets[0].payload.as_slice().to_vec();
    defragmenter
        .push(packets[0].clone(), &first, [3; 32])
        .unwrap();

    let mut contradictory = packets[1].clone();
    contradictory.fragment_count += 1;
    let second = contradictory.payload.as_slice().to_vec();
    assert_eq!(
        defragmenter                .push(contradictory, &second, [4; 32])
            .err()
            .unwrap(),
        PacketError::InvalidFragmentMetadata
    );
}

#[test]
fn single_fragment_is_delivered_immediately() {
    let mut defragmenter = Defragmenter::new();
    let single = packet(b"short");
    let message = defragmenter
        .push(single, b"short", [9; 32])
        .unwrap()
        .expect("single fragment completes at once");
    assert_eq!(message.payload.as_slice(), b"short");
    assert_eq!(message.accepted_fragments.len(), 1);
}

// ---- storage encoding ---------------------------------------------------

#[test]
fn conversation_peer_and_storage_ids() {
    assert!(ConversationId::from_peer(BROADCAST_RECIPIENT).is_broadcast());
    assert_eq!(
        ConversationId::Broadcast.peer_id(),
        BROADCAST_RECIPIENT,
        "the conversation layer identifies broadcast by the zero peer"
    );
    assert_eq!(
        ConversationId::Broadcast.storage_id(),
        BROADCAST_STORAGE_ID,
        "storage uses its own reserved key"
    );

    let direct = ConversationId::from_peer(id(0x42));
    assert!(!direct.is_broadcast());
    assert_eq!(direct.peer_id(), id(0x42));
    assert_eq!(direct.storage_id(), id(0x42));
    assert_ne!(BROADCAST_STORAGE_ID, BROADCAST_RECIPIENT);
}

#[test]
fn stored_message_text_round_trip() {
    let original = stored_text();
    let mut buffer = [0u8; 640];
    let written = original.serialize_into(&mut buffer).unwrap();
    let (decoded, consumed) = StoredMessage::deserialize_from(&buffer).unwrap();
    assert_eq!(consumed, written);
    assert_eq!(decoded, original);
}

#[test]
fn stored_message_coordinate_round_trip() {
    let original = StoredMessage::new(
        3,
        false,
        999,
        MessageType::Coords {
            latitude_microdegrees: 470_000_000,
            longitude_microdegrees: 80_000_000,
        },
        "",
        11,
        12,
        id(0x44),
        false,
    );
    let mut buffer = [0u8; 640];
    original.serialize_into(&mut buffer).unwrap();
    let (decoded, _) = StoredMessage::deserialize_from(&buffer).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(
        decoded.kind.coordinates(),
        Some((470_000_000, 80_000_000))
    );
}

#[test]
fn stored_message_rejects_unknown_kind() {
    let original = stored_text();
    let mut buffer = [0u8; 640];
    original.serialize_into(&mut buffer).unwrap();
    // kind tag follows ordinal, flag, timestamp, packet ID, message ID, sender.
    buffer[34] = 0x7F;
    assert!(StoredMessage::deserialize_from(&buffer).is_err());
}

#[test]
fn stored_message_rejects_oversized_text_length() {
    let original = stored_text();
    let mut buffer = [0u8; 640];
    original.serialize_into(&mut buffer).unwrap();
    let too_long = (u16::try_from(MAX_MESSAGE_PAYLOAD).unwrap() + 1).to_le_bytes();
    buffer[35..37].copy_from_slice(&too_long);
    assert!(StoredMessage::deserialize_from(&buffer).is_err());
}

#[test]
fn contact_round_trip_and_display_name() {
    let original = contact(0x55, "alice", 0xAB, true);
    let mut buffer = [0u8; 128];
    let written = original.serialize_into(&mut buffer).unwrap();
    let (decoded, consumed) = Contact::deserialize_from(&buffer).unwrap();
    assert_eq!(consumed, written);
    assert_eq!(decoded, original);
    assert_eq!(decoded.display_name().as_str(), "alice");

    let unnamed = contact(0x55, "", 0, false);
    let label = unnamed.display_name();
    assert_eq!(label.len(), DEVICE_ID_CHARS);
    assert_eq!(label, hex_device_id(&id(0x55)));
}

#[test]
fn storage_keys_are_namespaced_and_distinct() {
    let device = id(0x11);
    let setting = StorageKey::setting("display_brightness").unwrap();
    let contact_key = StorageKey::contact(&device);
    let message = StorageKey::message(&device, 3);
    let count = StorageKey::message_count(&device);
    let base = StorageKey::message_base(&device);

    assert!(contact_key.is_contact());
    assert!(!setting.is_contact());
    assert!(!message.is_contact());

    for key in [&message, &count, &base] {
        assert!(key.is_conversation_message());
    }
    assert!(!setting.is_conversation_message());
    assert!(!contact_key.is_conversation_message());

    // Records at different ordinals are different keys.
    assert_ne!(message, StorageKey::message(&device, 4));
    assert_ne!(message, count);
    assert_ne!(count, base);
    assert_ne!(setting, StorageKey::setting("timezone_offset_minutes").unwrap());
}

#[test]
fn storage_key_rejects_long_settings() {
    let longest = "a".repeat(31);
    assert!(StorageKey::setting(&longest).is_some());
    let too_long = "a".repeat(32);
    assert!(StorageKey::setting(&too_long).is_none());
    assert!(StorageKey::setting("").is_none());
}

#[test]
fn storage_key_round_trips() {
    let key = StorageKey::message(&id(0x22), 0xDEAD_BEEF);
    let mut buffer = [0u8; 32];
    let written = key.serialize_into(&mut buffer).unwrap();
    assert_eq!(written, StorageKey::get_len(&buffer).unwrap());

    let (decoded, consumed) = StorageKey::deserialize_from(&buffer).unwrap();
    assert_eq!(consumed, written);
    assert_eq!(decoded, key);
}

// ---- radio configuration ------------------------------------------------

#[test]
fn frequency_bands_cover_expected_ranges() {
    assert_eq!(
        FrequencyBand::for_frequency(869_400_000),
        Some(FrequencyBand::SubGhz)
    );
    assert_eq!(
        FrequencyBand::for_frequency(150_000_000),
        Some(FrequencyBand::SubGhz)
    );
    assert_eq!(
        FrequencyBand::for_frequency(2_000_000_000),
        Some(FrequencyBand::SBand)
    );
    assert_eq!(
        FrequencyBand::for_frequency(2_400_000_000),
        Some(FrequencyBand::Ism2_4Ghz)
    );
    assert_eq!(FrequencyBand::for_frequency(100_000_000), None);
    assert_eq!(FrequencyBand::for_frequency(1_000_000_000), None);

    assert!(FrequencyBand::SubGhz.contains(869_400_000));
    assert!(!FrequencyBand::SubGhz.contains(2_400_000_000));
    assert!(FrequencyBand::SBand.contains(2_000_000_000));
}

#[test]
fn band_packet_sizes() {
    assert_eq!(FrequencyBand::SubGhz.maximum_packet_size(), 255);
    assert_eq!(FrequencyBand::Ism2_4Ghz.maximum_packet_size(), 255);
    assert_eq!(FrequencyBand::SBand.maximum_packet_size(), 133);
}

#[test]
fn lora_bandwidth_encodings() {
    assert_eq!(LoraBandwidth::Khz62_5.hz(), 62_500);
    assert_eq!(LoraBandwidth::Khz125.hz(), 125_000);
    assert_eq!(LoraBandwidth::Khz812.hz(), 812_000);

    assert_eq!(
        LoraBandwidth::Khz125.lr1121_encoding(FrequencyBand::SubGhz),
        Some(0x04)
    );
    assert_eq!(
        LoraBandwidth::Khz203.lr1121_encoding(FrequencyBand::SubGhz),
        None
    );
    assert_eq!(
        LoraBandwidth::Khz203.lr1121_encoding(FrequencyBand::Ism2_4Ghz),
        Some(0x0D)
    );
    assert_eq!(
        LoraBandwidth::Khz812.lr1121_encoding(FrequencyBand::Ism2_4Ghz),
        Some(0x0F)
    );
    assert_eq!(
        LoraBandwidth::Khz125.lr1121_encoding(FrequencyBand::SBand),
        None
    );
}

#[test]
fn coding_rates_encode_in_order() {
    assert_eq!(CodingRate::Cr45.register_value(), 0x01);
    assert_eq!(CodingRate::Cr46.register_value(), 0x02);
    assert_eq!(CodingRate::Cr47.register_value(), 0x03);
    assert_eq!(CodingRate::Cr48.register_value(), 0x04);
}

#[test]
fn pa_modes_cap_power() {
    assert_eq!(PaMode::LowPower.maximum_tx_power_dbm(), 14);
    assert_eq!(PaMode::HighPower.maximum_tx_power_dbm(), 22);
    assert_eq!(PaMode::HighFrequencyPath.maximum_tx_power_dbm(), 13);
    assert_eq!(
        PaMode::LowPower.lr1121_register_values(),
        PaMode::HighPower.lr1121_register_values()
    );
    assert_ne!(
        PaMode::HighPower.lr1121_register_values(),
        PaMode::HighFrequencyPath.lr1121_register_values()
    );
}

#[test]
fn radio_config_defaults_per_band() {
    let sub = RadioConfig::new(869_400_000);
    assert_eq!(sub.frequency_band, FrequencyBand::SubGhz);
    assert_eq!(sub.pa_mode, PaMode::LowPower);
    assert_eq!(sub.spreading_factor, 9);
    assert_eq!(sub.bandwidth, LoraBandwidth::Khz125);
    assert_eq!(sub.coding_rate, CodingRate::Cr45);
    assert_eq!(sub.preamble_symbols, 8);
    assert!(sub.crc_enabled);
    assert!(!sub.implicit_header);
    assert!(!sub.invert_iq);
    assert_eq!(sub.low_data_rate_optimization, None);
    assert_eq!(sub.validate(), Ok(()));

    assert_eq!(
        RadioConfig::new(2_400_000_000).pa_mode,
        PaMode::HighFrequencyPath
    );
    assert_eq!(RadioConfig::new(2_000_000_000).frequency_band, FrequencyBand::SBand);
    assert_eq!(RadioConfig::default().frequency_hz, 869_500_000);
}

#[test]
fn radio_config_validation_rejects_bad_values() {
    let mut config = RadioConfig::new(869_400_000);

    config.spreading_factor = 4;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::InvalidSpreadingFactor
    );
    config.spreading_factor = 13;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::InvalidSpreadingFactor
    );
    config.spreading_factor = 9;

    config.bandwidth = LoraBandwidth::Khz203;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::UnsupportedBandwidth
    );
    config.bandwidth = LoraBandwidth::Khz125;

    config.preamble_symbols = 0;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::InvalidPreambleLength
    );
    config.preamble_symbols = 8;

    config.tx_power_dbm = 30;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::InvalidTransmitPower
    );
    config.tx_power_dbm = 10;

    config.pa_mode = PaMode::HighFrequencyPath;
    assert_eq!(config.validate().unwrap_err(), ConfigError::UnsupportedPaMode);
    config.pa_mode = PaMode::LowPower;

    config.frequency_hz = 2_400_000_000;
    assert_eq!(
        config.validate().unwrap_err(),
        ConfigError::UnsupportedFrequency
    );
}

#[test]
fn ldro_recommendation_depends_on_symbol_time() {
    let mut config = RadioConfig::new(869_400_000);
    config.spreading_factor = 9;
    config.bandwidth = LoraBandwidth::Khz125;
    assert!(!config.recommended_ldro());
    assert!(!config.low_data_rate_optimization_enabled());

    config.spreading_factor = 12;
    assert!(config.recommended_ldro());
    assert!(config.low_data_rate_optimization_enabled());

    config.low_data_rate_optimization = Some(false);
    assert!(!config.low_data_rate_optimization_enabled());
}

#[test]
fn time_on_air_grows_with_spreading_factor() {
    let mut config = RadioConfig::new(869_400_000);
    config.spreading_factor = 7;
    let fast = config.time_on_air_us(50);
    config.spreading_factor = 12;
    let slow = config.time_on_air_us(50);
    assert!(fast > 0);
    assert!(slow > fast, "slow={slow} fast={fast}");

    // S-band takes the LR-FHSS estimate rather than the LoRa formula.
    let sband = RadioConfig::new(2_000_000_000);
    assert!(sband.time_on_air_us(50) > 0);
}

#[test]
fn lr_fhss_profile_values() {
    let profile = LrFhssProfile::SBandDefault;
    assert_eq!(profile.bandwidth().hz(), 136_720);
    assert_eq!(profile.max_payload_size(), 133);
    assert_eq!(
        profile.header_count(),
        crate::radio::types::LrFhssHeaderCount::Two
    );
    assert_eq!(
        profile.coding_rate(),
        crate::radio::types::LrFhssCodingRate::Cr5_6
    );
    assert_eq!(
        profile.hopping(),
        crate::radio::types::LrFhssHopping::Enabled
    );
}

// ---- navigation ---------------------------------------------------------

#[test]
fn navigation_same_point() {
    let (distance, bearing) = distance_and_bearing((0, 0), (0, 0)).unwrap();
    assert!(distance.abs() < 1e-6);
    assert_eq!(bearing, 0);
}

#[test]
fn navigation_cardinal_bearings() {
    let (north_distance, north) = distance_and_bearing((0, 0), (1_000_000, 0)).unwrap();
    assert_eq!(north, 0);
    assert!((north_distance - 111_195.0).abs() < 50.0);

    let (east_distance, east) = distance_and_bearing((0, 0), (0, 1_000_000)).unwrap();
    assert_eq!(east, 90);
    assert!((east_distance - 111_195.0).abs() < 50.0);

    let (_, south) = distance_and_bearing((0, 0), (-1_000_000, 0)).unwrap();
    assert_eq!(south, 180);

    let (_, west) = distance_and_bearing((0, 0), (0, -1_000_000)).unwrap();
    assert_eq!(west, 270);
}

#[test]
fn compass_direction_boundaries() {
    assert_eq!(compass_direction(0), "N");
    assert_eq!(compass_direction(22), "N");
    assert_eq!(compass_direction(23), "NE");
    assert_eq!(compass_direction(90), "E");
    assert_eq!(compass_direction(180), "S");
    assert_eq!(compass_direction(270), "W");
    assert_eq!(compass_direction(337), "NW");
    assert_eq!(compass_direction(359), "N");
}

#[test]
fn relative_turn_is_shortest() {
    assert_eq!(relative_turn_degrees(90, 0), 90);
    assert_eq!(relative_turn_degrees(0, 90), -90);
    assert_eq!(relative_turn_degrees(350, 10), -20);
    assert_eq!(relative_turn_degrees(10, 350), 20);
    assert_eq!(relative_turn_degrees(0, 180).abs(), 180);
}

// ---- UI contact book ----------------------------------------------------

#[test]
fn contact_labels_for_known_and_unknown_peers() {
    let broadcast = ContactEntry::broadcast();
    assert_eq!(broadcast.list_label().as_str(), "BROADCAST [PUBLIC]");
    assert!(!broadcast.is_deletable());
    assert!(!broadcast.is_unknown());

    let paired = ContactEntry::paired(id(0x12), "Alice", [1; KEY_SIZE], false);
    assert_eq!(paired.list_label().as_str(), "Alice");
    assert_eq!(paired.name_or_id().as_str(), "Alice");
    assert!(!paired.is_unknown());

    let unnamed = ContactEntry::paired(id(0x12), "", [1; KEY_SIZE], false);
    assert!(unnamed.list_label().starts_with("PAIRED "));
    assert!(unnamed.list_label().ends_with("..."));
    assert_eq!(unnamed.name_or_id(), hex_device_id(&id(0x12)));

    let unknown = ContactEntry::unknown(id(0x34));
    assert!(unknown.list_label().starts_with("UNKNOWN "));
    assert!(unknown.is_unknown());
    assert_eq!(unknown.name_or_id(), hex_device_id(&id(0x34)));
}

#[test]
fn contact_book_adopts_and_updates() {
    let mut book = ContactBook::new();
    assert_eq!(book.entries().len(), 1);

    assert_eq!(book.adopt(&contact(0x11, "a", 1, false)), Ok(()));
    assert_eq!(book.adopt(&contact(0x22, "b", 2, false)), Ok(()));
    assert_eq!(book.entries().len(), 3);
    assert!(book.direct_key(&id(0x11)).is_some());
    assert!(book.entry_by_conversation(ConversationId::Direct(id(0x22))).is_some());

    // Adopting the same peer again updates in place.
    assert_eq!(book.adopt(&contact(0x11, "renamed", 9, true)), Ok(()));
    assert_eq!(book.entries().len(), 3);
    let entry = book
        .entry_by_conversation(ConversationId::Direct(id(0x11)))
        .unwrap();
    assert_eq!(entry.custom_name.as_str(), "renamed");
    assert!(entry.blocked);

    let reserved = Contact {
        device_id: [0; 16],
        key: [0; KEY_SIZE],
        custom_name: String::new(),
        blocked: false,
    };
    assert_eq!(
        book.adopt(&reserved),
        Err(crate::ui::contact::ContactBookError::ReservedBroadcastId)
    );
}

#[test]
fn contact_book_renames_and_registers_peers() {
    let mut book = ContactBook::new();
    book.adopt(&contact(0x11, "old", 1, false)).unwrap();

    book.set_name(id(0x11), "new");
    assert_eq!(
        book.entry_by_conversation(ConversationId::Direct(id(0x11)))
            .unwrap()
            .custom_name
            .as_str(),
        "new"
    );

    assert!(book.note_unknown(id(0x77)));
    assert!(!book.note_unknown(id(0x77)), "second note must be ignored");
    assert_eq!(
        book.entry_by_conversation(ConversationId::Direct(id(0x77)))
            .unwrap()
            .list_label()
            .as_str(),
        "UNKNOWN 7777..."
    );
}

#[test]
fn contact_book_blocks_and_removes() {
    let mut book = ContactBook::new();
    book.adopt(&contact(0x11, "a", 1, false)).unwrap();
    book.adopt(&contact(0x22, "b", 2, false)).unwrap();

    assert_eq!(book.visible_indices(false).len(), 3);
    assert_eq!(book.visible_indices(true).len(), 2);

    assert!(book.set_blocked(&id(0x11), true));
    assert_eq!(book.visible_indices(false).len(), 2);
    assert!(!book.set_blocked(&id(0x99), true), "unknown peer cannot be blocked");

    assert_eq!(
        book.remove(0),
        Err(crate::ui::contact::ContactBookError::BroadcastCannotBeDeleted)
    );
    assert_eq!(
        book.remove(99),
        Err(crate::ui::contact::ContactBookError::InvalidIndex)
    );
    assert!(book.remove(1).is_ok());
    assert!(book.entry_by_conversation(ConversationId::Direct(id(0x11))).is_none());
}

#[test]
fn contact_book_sorts_chats_by_activity() {
    let mut book = ContactBook::new();
    book.adopt(&contact(0x11, "a", 1, false)).unwrap();
    book.adopt(&contact(0x22, "b", 2, false)).unwrap();

    book.seed_preview(ConversationId::Direct(id(0x11)), "one", true, false, 100);
    book.seed_preview(ConversationId::Direct(id(0x22)), "two", false, false, 200);
    assert_eq!(book.chat_indices().as_slice(), &[2, 1, 0]);

    book.seed_preview(ConversationId::Direct(id(0x22)), "older", false, false, 50);
    assert_eq!(book.chat_indices().as_slice(), &[1, 2, 0]);

    // An unknown conversation is ignored rather than inserted.
    book.seed_preview(ConversationId::Direct(id(0x99)), "ignored", false, false, 999);
    assert_eq!(book.chat_indices().as_slice(), &[1, 2, 0]);
}

#[test]
fn preview_text_truncates_and_marks_gps() {
    assert_eq!(preview_text("short", None).as_str(), "short");

    let long = "a".repeat(40);
    let preview = preview_text(&long, None);
    assert_eq!(preview.len(), 22);
    assert!(preview.ends_with(".."));
    assert_eq!(preview.chars().filter(|c| *c == 'a').count(), 20);

    assert_eq!(
        preview_text("ignored", Some((1, 2))).as_str(),
        "[GPS]"
    );
}

// ---- notifications ------------------------------------------------------

#[test]
fn notification_flow() {
    // Drain anything a previous case might have left behind.
    let receiver = notify::POPUP_QUEUE.receiver();
    while receiver.try_receive().is_ok() {}

    let before = notify::history_revision();
    notify::history_changed();
    assert_eq!(notify::history_revision(), before + 1);

    notify::message_received(ConversationId::Direct(id(1)), id(1), "hello");
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::NewMessage);
    assert_eq!(popup.conversation, ConversationId::Direct(id(1)));
    assert_eq!(popup.sender_id, id(1));
    assert_eq!(popup.text.as_str(), "hello");
    assert!(popup.name.is_empty());
    assert_eq!(notify::history_revision(), before + 2);

    // Long text is cut to 31 characters plus an ellipsis.
    let long = "b".repeat(40);
    notify::message_received(ConversationId::Broadcast, id(2), &long);
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.text.len(), 34);
    assert!(popup.text.ends_with("..."));
    assert_eq!(popup.text.chars().filter(|c| *c == 'b').count(), 31);

    notify::storage_full(ConversationId::Broadcast);
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::Alert);
    assert_eq!(popup.text.as_str(), "STORAGE FULL");
    assert_eq!(popup.sender_id, [0; 16]);

    notify::send_queue_full(ConversationId::Direct(id(3)));
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::Alert);
    assert_eq!(popup.text.as_str(), "SEND QUEUE FULL");
    assert_eq!(popup.conversation, ConversationId::Direct(id(3)));

    notify::contact_request(id(4));
    let popup = receiver.try_receive().unwrap();
    assert_eq!(popup.popup_type, PopupType::ContactRequest);
    assert_eq!(popup.text.as_str(), "WANTS TO ADD YOU");
    assert_eq!(popup.conversation, ConversationId::Direct(id(4)));
    assert_eq!(popup.sender_id, id(4));

    // The queue holds eight; a ninth message drops the oldest.
    for index in 0..9u8 {
        let text = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight"]
            [index as usize];
        notify::message_received(ConversationId::Broadcast, id(5), text);
    }
    let first = receiver.try_receive().unwrap();
    assert_eq!(first.text.as_str(), "one", "oldest toast is dropped");
    let mut remaining = 0;
    while receiver.try_receive().is_ok() {
        remaining += 1;
    }
    assert_eq!(remaining, 7);
}

// ---- outbox -------------------------------------------------------------

#[test]
fn outbox_lifecycle() {
    assert_eq!(outbox::MAX_OUTBOX_SLOTS, 2);

    let mut one = Vec::<PacketData, MAX_FRAGMENT_COUNT>::new();
    assert!(one.push(packet(b"one")).is_ok());

    // Broadcast-style sends clear themselves once fully transmitted.
    outbox::submit(0x100, one.clone(), false).unwrap();
    assert_eq!(outbox::pump(0, true, |_| true), 0);

    // Acknowledged sends wait for a retry.
    let mut two = Vec::<PacketData, MAX_FRAGMENT_COUNT>::new();
    assert!(two.push(packet(b"two")).is_ok());
    outbox::submit(0x200, two.clone(), true).unwrap();
    let mut sends = 0;
    assert_eq!(
        outbox::pump(0, true, |_| {
            sends += 1;
            true
        }),
        1
    );
    assert_eq!(sends, 1);

    // Before the retry interval nothing is resent.
    sends = 0;
    assert_eq!(
        outbox::pump(19_999_999, true, |_| {
            sends += 1;
            true
        }),
        1
    );
    assert_eq!(sends, 0);

    // At the retry tick the slot is rearmed without sending.
    assert_eq!(outbox::pump(20_000_000, true, |_| true), 1);

    // The next call resends the whole message.
    sends = 0;
    assert_eq!(
        outbox::pump(20_000_001, true, |_| {
            sends += 1;
            true
        }),
        1
    );
    assert_eq!(sends, 1);

    // Third attempt, then the retry timer gives up on the fourth tick.
    assert_eq!(outbox::pump(40_000_001, true, |_| true), 1);
    assert_eq!(outbox::pump(40_000_002, true, |_| true), 1);
    assert_eq!(outbox::pump(60_000_002, true, |_| true), 0);

    // An acknowledgement drops a waiting slot immediately.
    outbox::submit(0x300, two.clone(), true).unwrap();
    outbox::acknowledge(0x300);
    assert_eq!(outbox::pump(0, true, |_| true), 0);

    // Two slots fill, the third message is refused.
    outbox::submit(0x401, one.clone(), true).unwrap();
    outbox::submit(0x402, one.clone(), true).unwrap();
    assert!(outbox::submit(0x403, one, true).is_err());

    outbox::acknowledge(0x401);
    outbox::acknowledge(0x402);
    assert_eq!(outbox::pump(0, true, |_| true), 0);
}

// ---- crypto constant ----------------------------------------------------

#[test]
fn crypto_key_size_matches_firmware() {
    let source = include_str!("../../src/bin/crypto/mod.rs");
    assert!(
        source.contains("pub const KEY_SIZE: usize = 32;"),
        "host-tests/src/lib.rs mirrors KEY_SIZE; update it when the firmware changes"
    );
    assert_eq!(KEY_SIZE, 32);
}
