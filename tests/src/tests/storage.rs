use super::*;

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
