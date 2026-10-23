use heapless::{Vec, index_map::FnvIndexMap};

use crate::packet::{
    packet_data::{DEFAULT_HOP_LIMIT, GCM_OVERHEAD, MAX_PAYLOAD_SIZE, PacketData},
    packet_error::PacketError,
    packet_type::PacketType,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FragmentKey {
    pub from: [u8; 16],
    pub message_id: u16,
}

pub struct Fragmenter;

impl Fragmenter {
    pub fn fragment(
        packet_id: u16,
        message_id: u16,
        from: [u8; 16],
        to: [u8; 16],
        is_encrypted: bool,
        packet_type: PacketType,
        payload: &[u8],
        initial_hop_limit: u8,
        hop_limit: u8,
    ) -> Result<Vec<PacketData, 16>, PacketError> {
        let overhead = if is_encrypted { GCM_OVERHEAD } else { 0 };
        if MAX_PAYLOAD_SIZE < overhead {
            return Err(PacketError::PayloadTooLarge);
        }

        let max_fragment_payload = MAX_PAYLOAD_SIZE - overhead;
        if max_fragment_payload == 0 {
            return Err(PacketError::PayloadTooLarge);
        }

        let fragment_count = if payload.is_empty() {
            1
        } else {
            ((payload.len() + max_fragment_payload - 1) / max_fragment_payload) as u8
        };

        if fragment_count == 0 {
            return Err(PacketError::InvalidFragmentMetadata);
        }

        if fragment_count as usize > 16 {
            return Err(PacketError::PayloadTooLarge);
        }

        let mut packets: Vec<PacketData, 16> = Vec::new();
        for index in 0..fragment_count {
            let start = (index as usize) * max_fragment_payload;
            let end = core::cmp::min(start + max_fragment_payload, payload.len());
            let chunk = &payload[start..end];

            let mut fragment_payload: Vec<u8, 212> = Vec::new();
            for byte in chunk {
                fragment_payload
                    .push(*byte)
                    .map_err(|_| PacketError::PayloadTooLarge)?;
            }

            let packet = PacketData::new(
                packet_id,
                message_id,
                index,
                fragment_count,
                from,
                to,
                is_encrypted,
                packet_type,
                fragment_payload.as_slice(),
                initial_hop_limit,
                hop_limit,
            )?;
            packets.push(packet).map_err(|_| PacketError::PayloadTooLarge)?;
        }

        Ok(packets)
    }

    pub fn fragment_with_defaults(
        packet_id: u16,
        message_id: u16,
        from: [u8; 16],
        to: [u8; 16],
        is_encrypted: bool,
        packet_type: PacketType,
        payload: &[u8],
    ) -> Result<Vec<PacketData, 16>, PacketError> {
        Self::fragment(
            packet_id,
            message_id,
            from,
            to,
            is_encrypted,
            packet_type,
            payload,
            DEFAULT_HOP_LIMIT,
            DEFAULT_HOP_LIMIT,
        )
    }
}

#[derive(Clone)]
struct FragmentEntry {
    total: u8,
    received: u8,
    fragments: [Option<PacketData>; 16],
}

pub struct Defragmenter {
    cache: FnvIndexMap<(FragmentKey, u16), FragmentEntry, 8>,
}

impl Defragmenter {
    pub fn new() -> Self {
        Self {
            cache: FnvIndexMap::new(),
        }
    }

    pub fn push(&mut self, packet: PacketData) -> Option<PacketData> {
        if packet.fragment_count == 1 {
            return Some(packet);
        }

        if packet.fragment_index >= packet.fragment_count {
            return None;
        }

        let key = FragmentKey {
            from: packet.from,
            message_id: packet.message_id,
        };
        let cache_key = (key, packet.packet_id);

        if !self.cache.contains_key(&cache_key) {
            self.cache
                .insert(
                    cache_key,
                    FragmentEntry {
                        total: packet.fragment_count,
                        received: 0,
                        fragments: [const { None }; 16],
                    },
                )
                .ok()?;
        }

        let idx = packet.fragment_index as usize;
        let total = packet.fragment_count;
        let entry = self.cache.get_mut(&cache_key)?;
        if entry.total != total {
            return None;
        }
        if entry.fragments[idx].is_some() {
            return None;
        }

        entry.fragments[idx] = Some(packet);
        entry.received += 1;

        if entry.received == entry.total {
            let entry = self.cache.remove(&cache_key)?;
            let mut total_len: usize = 0;
            for i in 0..entry.total {
                if let Some(frag) = &entry.fragments[i as usize] {
                    total_len += frag.payload.len();
                }
            }
            if total_len > MAX_PAYLOAD_SIZE {
                return None;
            }

            let first = entry.fragments[0].as_ref()?.clone();
            let mut payload: Vec<u8, 212> = Vec::new();
            for i in 0..entry.total {
                let frag = entry.fragments[i as usize].as_ref()?;
                for byte in frag.payload.iter() {
                    payload.push(*byte).ok()?;
                }
            }

            let mut merged = PacketData::new(
                first.packet_id,
                first.message_id,
                0,
                1,
                first.from,
                first.to,
                first.is_encrypted,
                first.packet_type,
                payload.as_slice(),
                first.initial_hop_limit,
                first.hop_limit,
            )
            .ok()?;
            merged.payload = payload;
            Some(merged)
        } else {
            None
        }
    }
}

impl Default for Defragmenter {
    fn default() -> Self {
        Self::new()
    }
}
