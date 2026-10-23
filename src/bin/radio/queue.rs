use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use heapless::Vec;

use crate::packet::{
    packet_data::{MAX_PACKET_SIZE, PacketData},
    packet_encoder::PacketEncoder,
    packet_error::PacketError,
};

pub const QUEUE_CAPACITY: usize = 4;
const FREQUENCY_CHANGE_CAPACITY: usize = 1;

pub type Packet = Vec<u8, MAX_PACKET_SIZE>;

static PACKETS: Channel<CriticalSectionRawMutex, Packet, QUEUE_CAPACITY> = Channel::new();
static FREQUENCY_CHANGES: Channel<CriticalSectionRawMutex, u32, FREQUENCY_CHANGE_CAPACITY> =
    Channel::new();

#[derive(Debug)]
pub enum QueueError {
    Packet(PacketError),
    PacketTooLong { length: usize, maximum: usize },
    Full,
}

pub fn add_packet(packet: &PacketData) -> Result<(), QueueError> {
    packet.validate().map_err(QueueError::Packet)?;

    let length = packet.encoded_len();
    if length > MAX_PACKET_SIZE {
        return Err(QueueError::PacketTooLong {
            length,
            maximum: MAX_PACKET_SIZE,
        });
    }

    let mut encoded = [0; MAX_PACKET_SIZE];
    let encoded_len = PacketEncoder::encode(packet, &mut encoded).map_err(QueueError::Packet)?;
    let packet =
        Packet::from_slice(&encoded[..encoded_len]).map_err(|_| QueueError::PacketTooLong {
            length: encoded_len,
            maximum: MAX_PACKET_SIZE,
        })?;

    PACKETS.try_send(packet).map_err(|_| QueueError::Full)
}

pub(crate) async fn next_packet() -> Packet {
    PACKETS.receive().await
}

pub(crate) fn queued_packet_count() -> usize {
    PACKETS.len()
}

pub(crate) fn queue_frequency_change(frequency_hz: u32) -> Result<(), ()> {
    FREQUENCY_CHANGES.try_send(frequency_hz).map_err(|_| ())
}

pub(crate) async fn next_frequency_change() -> u32 {
    FREQUENCY_CHANGES.receive().await
}
