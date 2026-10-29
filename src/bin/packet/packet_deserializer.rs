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

        // Reads the big-endian packet ID from bytes 1–4.
        let packet_id = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);

        // Reads the big-endian message ID from bytes 5–8.
        let message_id = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]);

        // Reads the fragment index and count from bytes 9–10.
        let fragment_index = bytes[9];
        let fragment_count = bytes[10];

        // Reads the sender device ID from bytes 11–26.
        let mut from = [0u8; 16];
        from.copy_from_slice(&bytes[11..27]);

        // Reads the recipient device ID from bytes 27–42.
        let mut to = [0u8; 16];
        to.copy_from_slice(&bytes[27..43]);

        // Reads the encryption flag from byte 43.
        let is_encrypted = match bytes[43] {
            0 => false,
            1 => true,
            _ => return Err(PacketError::InvalidEncryptionFlag),
        };

        // Reads the packet type from byte 44.
        let packet_type =
            PacketType::try_from(bytes[44]).map_err(|_| PacketError::InvalidPacketType)?;

        // Reads the sender's original hop limit from byte 45.
        let initial_hop_limit = bytes[45];

        // Reads the remaining hop limit from byte 46.
        let hop_limit = bytes[46];

        // Reads the payload starting at byte 47.
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
