use crate::packet::packet_data::{HEADER_SIZE, MAX_PACKET_SIZE, PacketData};
use crate::packet::packet_error::PacketError;

pub struct PacketEncoder;

impl PacketEncoder {
    pub fn encode(packet_data: &PacketData, buffer: &mut [u8]) -> Result<usize, PacketError> {
        let payload_len = packet_data.payload.len();
        let packet_len = HEADER_SIZE + payload_len;

        // Calculates the total encoded packet size.
        if packet_len > MAX_PACKET_SIZE {
            return Err(PacketError::PayloadTooLarge);
        }

        // Rejects output buffers that do not have enough room.
        if buffer.len() < packet_len {
            return Err(PacketError::BufferTooSmall);
        }

        // Stores the protocol version at byte 0.
        buffer[0] = packet_data.version;

        // Stores the big-endian packet ID across bytes 1–2.
        buffer[1..3].copy_from_slice(&packet_data.packet_id.to_be_bytes());

        // Stores the big-endian message ID across bytes 3–4.
        buffer[3..5].copy_from_slice(&packet_data.message_id.to_be_bytes());

        // Stores the fragment index and count in bytes 5–6.
        buffer[5] = packet_data.fragment_index;
        buffer[6] = packet_data.fragment_count;

        // Stores the sender device ID across bytes 7–22.
        buffer[7..23].copy_from_slice(&packet_data.from);

        // Stores the recipient device ID across bytes 23–38.
        buffer[23..39].copy_from_slice(&packet_data.to);

        // Stores the encryption flag at byte 39.
        buffer[39] = u8::from(packet_data.is_encrypted);

        // Stores the packet type at byte 40.
        buffer[40] = packet_data.packet_type as u8;

        // Stores the original hop limit at byte 41.
        buffer[41] = packet_data.initial_hop_limit;

        // Stores the remaining hop limit at byte 42.
        buffer[42] = packet_data.hop_limit;

        // Stores the packet payload starting at byte 43.
        buffer[HEADER_SIZE..packet_len].copy_from_slice(packet_data.payload.as_slice());

        Ok(packet_len)
    }
}
