use super::*;

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
