use core::sync::atomic::{AtomicU32, Ordering};

use heapless::Vec;

use crate::packet::packet_error::PacketError;
use crate::packet::packet_type::PacketType;

pub const PROTOCOL_VERSION: u8 = 2;

pub const HEADER_SIZE: usize = 47;

/// Header bytes covered by AES-GCM; the trailing hop limit is excluded.
pub const AUTHENTICATED_HEADER_SIZE: usize = HEADER_SIZE - 1;
pub const MAX_PACKET_SIZE: usize = 255;
pub const MAX_PAYLOAD_SIZE: usize = MAX_PACKET_SIZE - HEADER_SIZE;
pub const MAX_LOGICAL_MESSAGE_SIZE: usize = 512;
pub const MAX_FRAGMENT_COUNT: usize = 16;

pub const GCM_NONCE_SIZE: usize = 12;
pub const GCM_TAG_SIZE: usize = 16;
pub const GCM_OVERHEAD: usize = GCM_NONCE_SIZE + GCM_TAG_SIZE;

pub const DEFAULT_HOP_LIMIT: u8 = 3;

/// Allocate packet IDs from one counter shared by all local producers.
/// A batch reservation keeps fragments of one message consecutive and prevents
/// the UI, protocol responses, and diagnostic producer from colliding.
static LOCAL_PACKET_ID: AtomicU32 = AtomicU32::new(0);
/// First ID of the most recent reservation, reported as the last message ID.
static LAST_MESSAGE_ID: AtomicU32 = AtomicU32::new(0);

/// Set both counters: once at boot from flash, then from the reserved range.
pub fn set_local_packet_ids(next: u32, last_message_id: u32) {
    LOCAL_PACKET_ID.store(next, Ordering::Relaxed);
    LAST_MESSAGE_ID.store(last_message_id, Ordering::Relaxed);
}

/// Report the ID the next reservation starts from.
pub fn current_packet_id() -> u32 {
    LOCAL_PACKET_ID.load(Ordering::Relaxed)
}

/// Report the first ID of the most recent reservation.
pub fn last_message_id() -> u32 {
    LAST_MESSAGE_ID.load(Ordering::Relaxed)
}

#[derive(Clone)]
pub struct PacketData {
    // Stores the protocol version for this packet.
    pub version: u8,

    // Assigns a unique ID to this packet.
    pub packet_id: u32,

    // Groups all fragments that belong to one message.
    pub message_id: u32,

    // Stores this fragment's zero-based index.
    pub fragment_index: u8,

    // Stores the total fragment count for the message.
    pub fragment_count: u8,

    // Stores the sender device ID.
    pub from: [u8; 16],

    // Stores the recipient device ID; zeroed bytes mark broadcast traffic.
    pub to: [u8; 16],

    // Marks whether the payload is encrypted.
    pub is_encrypted: bool,

    // Stores the packet type carried in the payload.
    pub packet_type: PacketType,

    // Stores the payload bytes, including the nonce and tag when encrypted.
    pub payload: Vec<u8, MAX_PAYLOAD_SIZE>,

    // Stores the original hop limit set by the sender.
    pub initial_hop_limit: u8,

    // Tracks the remaining hops before dropping the packet.
    pub hop_limit: u8,
}

impl PacketData {
    pub fn new(
        packet_id: u32,
        message_id: u32,
        fragment_index: u8,
        fragment_count: u8,
        from: [u8; 16],
        to: [u8; 16],
        is_encrypted: bool,
        packet_type: PacketType,
        payload: &[u8],
        initial_hop_limit: u8,
        hop_limit: u8,
    ) -> Result<Self, PacketError> {
        // Rejects invalid fragment metadata.
        if fragment_count == 0
            || fragment_count as usize > MAX_FRAGMENT_COUNT
            || fragment_index >= fragment_count
        {
            return Err(PacketError::InvalidFragmentMetadata);
        }

        // Rejects payloads larger than the allowed packet size.
        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(PacketError::PayloadTooLarge);
        }

        // Rejects encrypted payloads that do not contain a nonce and tag.
        if is_encrypted && payload.len() < GCM_OVERHEAD {
            return Err(PacketError::InvalidEncryptedPayload);
        }

        let payload = Vec::from_slice(payload).map_err(|_| PacketError::PayloadTooLarge)?;

        Ok(Self {
            version: PROTOCOL_VERSION,
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
        })
    }

    pub fn validate(&self) -> Result<(), PacketError> {
        // Rejects unsupported protocol versions.
        if self.version != PROTOCOL_VERSION {
            return Err(PacketError::UnsupportedVersion);
        }

        // Rejects invalid fragment metadata.
        if self.fragment_count == 0
            || self.fragment_count as usize > MAX_FRAGMENT_COUNT
            || self.fragment_index >= self.fragment_count
        {
            return Err(PacketError::InvalidFragmentMetadata);
        }

        // Rejects payloads larger than the allowed packet size.
        if self.payload.len() > MAX_PAYLOAD_SIZE {
            return Err(PacketError::PayloadTooLarge);
        }

        // Rejects encrypted payloads that do not contain a nonce and tag.
        if self.is_encrypted && self.payload.len() < GCM_OVERHEAD {
            return Err(PacketError::InvalidEncryptedPayload);
        }

        // Rejects hop limits above the initial value.
        if self.hop_limit > self.initial_hop_limit {
            return Err(PacketError::InvalidHopLimit);
        }

        Ok(())
    }

    pub fn is_broadcast(&self) -> bool {
        // Detects broadcast traffic by checking for an all-zero recipient ID.
        self.to.iter().all(|&byte| byte == 0)
    }

    pub fn is_for(&self, device_id: &[u8; 16]) -> bool {
        // Matches packets addressed to this device or broadcast to everyone.
        self.to == *device_id || self.is_broadcast()
    }

    pub fn is_fragmented(&self) -> bool {
        // Detects packets split across multiple fragments.
        self.fragment_count > 1
    }

    pub fn encoded_len(&self) -> usize {
        // Calculates the full encoded size in bytes.
        HEADER_SIZE + self.payload.len()
    }

    /// Write the fixed header in wire order, including the mutable hop limit.
    pub(crate) fn write_header(&self, header: &mut [u8; HEADER_SIZE]) {
        // Stores the protocol version at byte 0.
        header[0] = self.version;
        // Stores the big-endian packet ID across bytes 1-4.
        header[1..5].copy_from_slice(&self.packet_id.to_be_bytes());
        // Stores the big-endian message ID across bytes 5-8.
        header[5..9].copy_from_slice(&self.message_id.to_be_bytes());
        // Stores the fragment index and count in bytes 9-10.
        header[9] = self.fragment_index;
        header[10] = self.fragment_count;
        // Stores the sender device ID across bytes 11-26.
        header[11..27].copy_from_slice(&self.from);
        // Stores the recipient device ID across bytes 27-42.
        header[27..43].copy_from_slice(&self.to);
        // Stores the encryption flag at byte 43.
        header[43] = u8::from(self.is_encrypted);
        // Stores the packet type at byte 44.
        header[44] = self.packet_type as u8;
        // Stores the original hop limit at byte 45.
        header[45] = self.initial_hop_limit;
        // Stores the remaining hop limit at byte 46; relays may lower it.
        header[46] = self.hop_limit;
    }

    /// Return the immutable header bytes used as AES-GCM associated data.
    pub fn authenticated_header(&self) -> [u8; AUTHENTICATED_HEADER_SIZE] {
        let mut header = [0; HEADER_SIZE];
        self.write_header(&mut header);
        // The hop limit is the last header byte and the only mutable field.
        let mut authenticated = [0; AUTHENTICATED_HEADER_SIZE];
        authenticated.copy_from_slice(&header[..AUTHENTICATED_HEADER_SIZE]);
        authenticated
    }

    pub fn application_payload_len(&self) -> Option<usize> {
        // Calculates the app payload size without the AES-GCM overhead.
        if self.is_encrypted {
            self.payload.len().checked_sub(GCM_OVERHEAD)
        } else {
            Some(self.payload.len())
        }
    }

    pub fn can_forward(&self) -> bool {
        // Allows forwarding while the packet still has hops available.
        self.hop_limit > 0
    }

    pub fn decrement_hop_limit(&mut self) -> bool {
        // Stops forwarding once the packet reaches its hop limit.
        if !self.can_forward() {
            return false;
        }

        self.hop_limit -= 1;
        true
    }
}
