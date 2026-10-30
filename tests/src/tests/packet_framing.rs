use super::*;

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
