use super::*;

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
