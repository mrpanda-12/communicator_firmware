use crate::{packet::packet_deserializer::PacketDeserializer, radio::queue::Packet};

pub struct PacketHandler;

impl PacketHandler {
    pub fn handle_packet(packet: Packet) {
        match PacketDeserializer::decode(packet.as_slice()) {
            Ok(decoded) => {
                esp_println::println!(
                    "RX {:?} id={} message={} encrypted={} hops={}/{} bytes={}",
                    decoded.packet_type,
                    decoded.packet_id,
                    decoded.message_id,
                    decoded.is_encrypted,
                    decoded.hop_limit,
                    decoded.initial_hop_limit,
                    packet.len(),
                );

                if !decoded.is_encrypted {
                    match core::str::from_utf8(decoded.payload.as_slice()) {
                        Ok(text) => esp_println::println!("RX payload: {}", text),
                        Err(_) => {
                            esp_println::println!("RX payload bytes: {:02X?}", decoded.payload)
                        }
                    }
                }
            }
            Err(_) => {
                esp_println::println!("RX invalid packet bytes: {:02X?}", packet);
            }
        }
    }
}
