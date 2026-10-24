use crate::packet::packet_data::{HEADER_SIZE, MAX_PACKET_SIZE, PROTOCOL_VERSION, PacketData};
use crate::packet::packet_error::PacketError;
use crate::packet::packet_type::PacketType;

pub struct PacketDeserializer;

impl PacketDeserializer {
    pub fn deserialize(bytes: &[u8]) -> Result<PacketData, PacketError> {
        // Rejects packets shorter than the minimum header size.
        if bytes.len() < HEADER_SIZE {
            return Err(PacketError::PacketTooShort);
        }

        // Rejects packets larger than the supported maximum.
        if bytes.len() > MAX_PACKET_SIZE {
            return Err(PacketError::PayloadTooLarge);
        }

        // Reads the protocol version from byte 0.
        let version = bytes[0];

        if version != PROTOCOL_VERSION {
            return Err(PacketError::UnsupportedVersion);
        }

        // Reads the big-endian packet ID from bytes 1–2.
        let packet_id = u16::from_be_bytes([bytes[1], bytes[2]]);

        // Reads the big-endian message ID from bytes 3–4.
        let message_id = u16::from_be_bytes([bytes[3], bytes[4]]);

        // Reads the fragment index and count from bytes 5–6.
        let fragment_index = bytes[5];
        let fragment_count = bytes[6];

        // Reads the sender device ID from bytes 7–22.
        let mut from = [0u8; 16];
        from.copy_from_slice(&bytes[7..23]);

        // Reads the recipient device ID from bytes 23–38.
        let mut to = [0u8; 16];
        to.copy_from_slice(&bytes[23..39]);

        // Reads the encryption flag from byte 39.
        let is_encrypted = match bytes[39] {
            0 => false,
            1 => true,
            _ => return Err(PacketError::InvalidEncryptionFlag),
        };

        // Reads the packet type from byte 40.
        let packet_type =
            PacketType::try_from(bytes[40]).map_err(|_| PacketError::InvalidPacketType)?;

        // Reads the sender's original hop limit from byte 41.
        let initial_hop_limit = bytes[41];

        // Reads the remaining hop limit from byte 42.
        let hop_limit = bytes[42];

        // Reads the payload starting at byte 43.
        let payload = &bytes[HEADER_SIZE..];

        let packet = PacketData::new(
            packet_id,
            message_id,
            fragment_index,
            fragment_count,
            from,
            to,
            is_encrypted,
            packet_type,
            payload,
            initial_hop_limit,
            hop_limit,
        )?;

        Ok(packet)
    }
}
