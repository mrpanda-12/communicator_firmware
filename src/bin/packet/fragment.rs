use heapless::Vec;

use crate::packet::{
    packet_data::{
        DEFAULT_HOP_LIMIT, GCM_OVERHEAD, HEADER_SIZE, MAX_FRAGMENT_COUNT, MAX_LOGICAL_MESSAGE_SIZE,
        MAX_PACKET_SIZE, MAX_PAYLOAD_SIZE, PacketData,
    },
    packet_error::PacketError,
    packet_type::PacketType,
};

const REASSEMBLY_CAPACITY: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FragmentKey {
    pub from: [u8; 16],
    pub to: [u8; 16],
    pub message_id: u16,
}

pub struct Fragmenter;

impl Fragmenter {
    /// Split plaintext into packets. Encrypted callers must encrypt every returned packet before queuing it.
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
    ) -> Result<Vec<PacketData, MAX_FRAGMENT_COUNT>, PacketError> {
        Self::fragment_with_limit(
            packet_id,
            message_id,
            from,
            to,
            is_encrypted,
            packet_type,
            payload,
            initial_hop_limit,
            hop_limit,
            MAX_PACKET_SIZE,
        )
    }

    /// Split plaintext using the encoded-packet limit of the active radio band.
    pub fn fragment_with_limit(
        packet_id: u16,
        message_id: u16,
        from: [u8; 16],
        to: [u8; 16],
        is_encrypted: bool,
        packet_type: PacketType,
        payload: &[u8],
        initial_hop_limit: u8,
        hop_limit: u8,
        maximum_packet_size: usize,
    ) -> Result<Vec<PacketData, MAX_FRAGMENT_COUNT>, PacketError> {
        // Reserve the current band's header and GCM overhead before splitting application text.
        if payload.len() > MAX_LOGICAL_MESSAGE_SIZE {
            return Err(PacketError::PayloadTooLarge);
        }
        if maximum_packet_size > MAX_PACKET_SIZE || maximum_packet_size < HEADER_SIZE {
            return Err(PacketError::PayloadTooLarge);
        }
        let overhead = if is_encrypted { GCM_OVERHEAD } else { 0 };
        let available_payload = maximum_packet_size - HEADER_SIZE;
        if available_payload < overhead {
            return Err(PacketError::PayloadTooLarge);
        }
        let max_fragment_payload = available_payload - overhead;
        if max_fragment_payload == 0 && !payload.is_empty() {
            return Err(PacketError::PayloadTooLarge);
        }

        let fragment_count = if payload.is_empty() {
            1
        } else {
            payload.len().div_ceil(max_fragment_payload)
        };

        if fragment_count == 0 || fragment_count > MAX_FRAGMENT_COUNT {
            return Err(PacketError::PayloadTooLarge);
        }

        let mut packets: Vec<PacketData, MAX_FRAGMENT_COUNT> = Vec::new();
        for index in 0..fragment_count {
            let start = index * max_fragment_payload;
            let end = core::cmp::min(start + max_fragment_payload, payload.len());
            let chunk = &payload[start..end];

            let mut packet = PacketData::new(
                packet_id.wrapping_add(index as u16),
                message_id,
                index as u8,
                fragment_count as u8,
                from,
                to,
                false,
                packet_type,
                chunk,
                initial_hop_limit,
                hop_limit,
            )?;
            // Build with plaintext first so a short final chunk can be encrypted to a valid GCM frame.
            packet.is_encrypted = is_encrypted;
            packets
                .push(packet)
                .map_err(|_| PacketError::PayloadTooLarge)?;
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
    ) -> Result<Vec<PacketData, MAX_FRAGMENT_COUNT>, PacketError> {
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

    pub fn fragment_with_defaults_and_limit(
        packet_id: u16,
        message_id: u16,
        from: [u8; 16],
        to: [u8; 16],
        is_encrypted: bool,
        packet_type: PacketType,
        payload: &[u8],
        maximum_packet_size: usize,
    ) -> Result<Vec<PacketData, MAX_FRAGMENT_COUNT>, PacketError> {
        Self::fragment_with_limit(
            packet_id,
            message_id,
            from,
            to,
            is_encrypted,
            packet_type,
            payload,
            DEFAULT_HOP_LIMIT,
            DEFAULT_HOP_LIMIT,
            maximum_packet_size,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcceptedFragment {
    pub packet_id: u16,
    pub fingerprint: [u8; 32],
}

pub struct ReassembledMessage {
    pub first_packet: PacketData,
    pub payload: Vec<u8, MAX_LOGICAL_MESSAGE_SIZE>,
    pub encrypted: bool,
    pub accepted_fragments: Vec<AcceptedFragment, MAX_FRAGMENT_COUNT>,
}

#[derive(Clone, Copy)]
struct FragmentMeta {
    packet_id: u16,
    fingerprint: [u8; 32],
    offset: usize,
    length: usize,
}

struct FragmentEntry {
    key: FragmentKey,
    total: u8,
    is_encrypted: bool,
    packet_type: PacketType,
    initial_hop_limit: u8,
    fragments: [Option<FragmentMeta>; MAX_FRAGMENT_COUNT],
    first_packet: Option<PacketData>,
    received: u8,
    payload: Vec<u8, MAX_LOGICAL_MESSAGE_SIZE>,
}

pub struct Defragmenter {
    cache: Vec<FragmentEntry, REASSEMBLY_CAPACITY>,
}

impl Defragmenter {
    pub fn new() -> Self {
        Self { cache: Vec::new() }
    }

    /// Store authenticated plaintext fragments and return only after a complete logical message exists.
    pub fn push(
        &mut self,
        packet: PacketData,
        plaintext: &[u8],
        fingerprint: [u8; 32],
    ) -> Result<Option<ReassembledMessage>, PacketError> {
        if packet.fragment_count == 0
            || packet.fragment_count as usize > MAX_FRAGMENT_COUNT
            || packet.fragment_index >= packet.fragment_count
            || plaintext.len() > MAX_PAYLOAD_SIZE
        {
            return Err(PacketError::InvalidFragmentMetadata);
        }

        let mut message_payload = Vec::new();
        message_payload
            .extend_from_slice(plaintext)
            .map_err(|_| PacketError::PayloadTooLarge)?;
        let mut accepted_fragments = Vec::new();
        accepted_fragments
            .push(AcceptedFragment {
                packet_id: packet.packet_id,
                fingerprint,
            })
            .map_err(|_| PacketError::PayloadTooLarge)?;

        if packet.fragment_count == 1 {
            return Ok(Some(ReassembledMessage {
                first_packet: packet.clone(),
                payload: message_payload,
                encrypted: packet.is_encrypted,
                accepted_fragments,
            }));
        }

        let key = FragmentKey {
            from: packet.from,
            to: packet.to,
            message_id: packet.message_id,
        };
        let index = packet.fragment_index as usize;
        let entry_index = match self.cache.iter().position(|entry| entry.key == key) {
            Some(index) => index,
            None => {
                if self.cache.is_full() {
                    self.cache.remove(0);
                }
                self.cache
                    .push(FragmentEntry {
                        key,
                        total: packet.fragment_count,
                        is_encrypted: packet.is_encrypted,
                        packet_type: packet.packet_type,
                        initial_hop_limit: packet.initial_hop_limit,
                        fragments: [const { None }; MAX_FRAGMENT_COUNT],
                        first_packet: None,
                        received: 0,
                        payload: Vec::new(),
                    })
                    .map_err(|_| PacketError::PayloadTooLarge)?;
                self.cache.len() - 1
            }
        };

        let consistent = {
            let entry = &self.cache[entry_index];
            entry.total == packet.fragment_count
                && entry.is_encrypted == packet.is_encrypted
                && entry.packet_type == packet.packet_type
                && entry.initial_hop_limit == packet.initial_hop_limit
        };
        if !consistent {
            self.cache.remove(entry_index);
            return Err(PacketError::InvalidFragmentMetadata);
        }

        let duplicate = self.cache[entry_index].fragments[index].as_ref();
        if let Some(existing) = duplicate {
            if existing.packet_id == packet.packet_id && existing.fingerprint == fingerprint {
                return Ok(None);
            }
            self.cache.remove(entry_index);
            return Err(PacketError::InvalidFragmentMetadata);
        }

        if self.cache[entry_index].payload.len() + plaintext.len() > MAX_LOGICAL_MESSAGE_SIZE {
            self.cache.remove(entry_index);
            return Err(PacketError::PayloadTooLarge);
        }

        let entry = &mut self.cache[entry_index];
        if packet.fragment_index == 0 {
            entry.first_packet = Some(packet.clone());
        }
        let offset = entry.payload.len();
        entry
            .payload
            .extend_from_slice(plaintext)
            .map_err(|_| PacketError::PayloadTooLarge)?;
        entry.fragments[index] = Some(FragmentMeta {
            packet_id: packet.packet_id,
            fingerprint,
            offset,
            length: plaintext.len(),
        });
        entry.received += 1;

        if entry.received == entry.total {
            let entry = self.cache.remove(entry_index);
            let mut payload = Vec::new();
            let mut accepted_fragments = Vec::new();
            let first_packet = entry.first_packet;
            for i in 0..entry.total {
                let Some(fragment) = entry.fragments[i as usize].as_ref() else {
                    return Err(PacketError::InvalidFragmentMetadata);
                };
                payload
                    .extend_from_slice(
                        &entry.payload[fragment.offset..fragment.offset + fragment.length],
                    )
                    .map_err(|_| PacketError::PayloadTooLarge)?;
                accepted_fragments
                    .push(AcceptedFragment {
                        packet_id: fragment.packet_id,
                        fingerprint: fragment.fingerprint,
                    })
                    .map_err(|_| PacketError::PayloadTooLarge)?;
            }

            Ok(Some(ReassembledMessage {
                first_packet: first_packet.ok_or(PacketError::InvalidFragmentMetadata)?,
                payload,
                encrypted: entry.is_encrypted,
                accepted_fragments,
            }))
        } else {
            Ok(None)
        }
    }
}

impl Default for Defragmenter {
    fn default() -> Self {
        Self::new()
    }
}
