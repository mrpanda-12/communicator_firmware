use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use heapless::Vec;

use crate::packet::{
    packet_data::{MAX_PACKET_SIZE, PacketData},
    packet_error::PacketError,
    packet_serializer::PacketSerializer,
};

pub const QUEUE_CAPACITY: usize = 16;

pub type Packet = Vec<u8, MAX_PACKET_SIZE>;

/// 1/ Hold the queues associated with one radio instance.
pub struct RadioQueue {
    packets: Channel<CriticalSectionRawMutex, Packet, QUEUE_CAPACITY>,
}

impl RadioQueue {
    /// 2/ Create an empty TX queue for one radio worker.
    pub const fn new() -> Self {
        Self {
            packets: Channel::new(),
        }
    }

    /// 3/ Validate and encode a packet before adding it to this radio's FIFO.
    pub fn add_packet(&self, packet: &PacketData) -> Result<(), QueueError> {
        packet.validate().map_err(QueueError::Packet)?;

        let length = packet.encoded_len();
        if length > MAX_PACKET_SIZE {
            return Err(QueueError::PacketTooLong {
                length,
                maximum: MAX_PACKET_SIZE,
            });
        }

        let mut encoded = [0; MAX_PACKET_SIZE];
        let encoded_len =
            PacketSerializer::serialize(packet, &mut encoded).map_err(QueueError::Packet)?;
        let packet =
            Packet::from_slice(&encoded[..encoded_len]).map_err(|_| QueueError::PacketTooLong {
                length: encoded_len,
                maximum: MAX_PACKET_SIZE,
            })?;

        self.packets.try_send(packet).map_err(|_| QueueError::Full)
    }

    /// 4/ Let the owning radio worker wait for its next encoded packet.
    pub(crate) async fn next_packet(&self) -> Packet {
        self.packets.receive().await
    }

    /// 5/ Report queued packets without counting a packet already transmitting.
    pub(crate) fn queued_packet_count(&self) -> usize {
        self.packets.len()
    }

}

impl Default for RadioQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum QueueError {
    Packet(PacketError),
    PacketTooLong { length: usize, maximum: usize },
    Full,
}
