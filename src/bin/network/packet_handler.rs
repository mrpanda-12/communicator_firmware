use core::str::from_utf8;
use embassy_time::Delay;
use embedded_hal::spi::SpiDevice;

use crate::{
    packet::{packet_data::PacketData, packet_deserializer::PacketDeserializer},
    radio::queue::{self, Packet, QueueError},
    storage::storage_manager::StorageManager,
};
use crate::packet::packet_type::PacketType;

const SEEN_PACKET_CACHE_CAPACITY: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
struct PacketKey {
    sender: [u8; 16],
    packet_id: u16,
}

pub struct PacketHandler {
    seen_packets: heapless::Vec<PacketKey, SEEN_PACKET_CACHE_CAPACITY>,
}

impl PacketHandler {
    pub const fn new() -> Self {
        Self {
            seen_packets: heapless::Vec::new(),
        }
    }

    pub async fn handle_packet<SPI>(
        &mut self,
        packet: Packet,
        storage: &mut StorageManager<'static, SPI, Delay>,
        flash_buffer: &mut [u8],
    ) where
        SPI: SpiDevice<u8>,
    {
        // Decode packet
        let mut packet: PacketData = match PacketDeserializer::decode(packet.as_slice()) {
            Ok(packet) => packet,
            Err(_) => {
                defmt::warn!("RX invalid packet ({} bytes)", packet.len());
                return;
            }
        };

        // Validate packet and return on error
        if packet.validate().is_err() {
            defmt::warn!("RX rejected packet with invalid fields");
            return;
        }

        // Get or create device id
        let device_id = match storage.load_device_id(flash_buffer).await {
            Ok(Some(device_id)) => device_id,
            Ok(None) => {
                defmt::warn!("RX ignored: this device has no stored identity");
                return;
            }
            Err(_) => {
                defmt::warn!("RX ignored: could not load this device's identity");
                return;
            }
        };

        // Drop our own packets if another node relays them back to us.
        if packet.from == device_id {
            defmt::debug!("RX ignored: packet originated from this device");
            return;
        }

        // Return if packet was in the last 16 packets
        let packet_key = PacketKey {
            sender: packet.from,
            packet_id: packet.packet_id,
        };
        if !self.remember_packet(packet_key) {
            defmt::debug!(
                "RX duplicate dropped: sender packet id={}",
                packet.packet_id
            );
            return;
        }

        let broadcast = packet.is_broadcast();
        let addressed_to_this_device = packet.is_for(&device_id);


        // Dev info
        defmt::info!(
            "RX from= {} type={} id={} message={} encrypted={} broadcast={} addressed_here={} hops={}/{} bytes={}",
            packet.from,
            packet.packet_type as u8,
            packet.packet_id,
            packet.message_id,
            packet.is_encrypted,
            broadcast,
            addressed_to_this_device,
            packet.hop_limit,
            packet.initial_hop_limit,
            packet.encoded_len(),
        );

        if !packet.is_encrypted {
            if packet.packet_type == PacketType::Text {
                match from_utf8(packet.payload.as_slice()) {
                    Ok(text) => defmt::info!("RX payload: {=str}", text),
                    Err(_) => defmt::info!("RX payload is not UTF-8 ({} bytes)", packet.payload.len()),
                }
            }
        }

        if broadcast {
            forward_packet(&mut packet);
            // todo!("Handle broadcast packet");
        }

        if !addressed_to_this_device {
            forward_packet(&mut packet);
            return;
        }

        // todo!("Handle packet addressed to this device")
    }

    // Remember a packet once and evict the oldest key when the cache fills.
    fn remember_packet(&mut self, key: PacketKey) -> bool {
        if self.seen_packets.contains(&key) {
            return false;
        }

        if self.seen_packets.is_full() {
            self.seen_packets.remove(0);
        }

        // The cache has a free slot after evicting its oldest key if needed.
        let _ = self.seen_packets.push(key);
        true
    }
}

// Decrement a packet's remaining hops before queueing it for forwarding.
fn forward_packet(packet: &mut PacketData) {
    if !packet.decrement_hop_limit() {
        defmt::debug!("Packet reached its hop limit; not forwarding");
        return;
    }

    match queue::add_packet(packet) {
        Ok(()) => {}
        Err(QueueError::Full) => {
            defmt::warn!("Could not forward packet: TX queue is full");
        }
        Err(QueueError::Packet(_)) => {
            defmt::warn!("Could not forward packet: validation failed");
        }
        Err(QueueError::PacketTooLong { length, maximum }) => {
            defmt::warn!(
                "Could not forward oversized packet: {} bytes (max {})",
                length,
                maximum
            );
        }
    }
}
