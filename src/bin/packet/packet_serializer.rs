use crate::packet::packet_data::{HEADER_SIZE, MAX_PACKET_SIZE, PacketData};
use crate::packet::packet_error::PacketError;

pub struct PacketSerializer;

impl PacketSerializer {
    pub fn serialize(packet_data: &PacketData, buffer: &mut [u8]) -> Result<usize, PacketError> {
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

        // Writes the fixed header through the same layout used for authentication.
        let mut header = [0; HEADER_SIZE];
        packet_data.write_header(&mut header);
        buffer[..HEADER_SIZE].copy_from_slice(&header);

        // Stores the packet payload starting at byte 47.
        buffer[HEADER_SIZE..packet_len].copy_from_slice(packet_data.payload.as_slice());

        Ok(packet_len)
    }
}
