use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    watch::{Receiver, Watch},
};

use crate::{
    crypto::{self, KEY_SIZE},
    network::types::{PairingEvent, PairingStage, PendingPairing},
    packet::{
        packet_data::{DEFAULT_HOP_LIMIT, GCM_OVERHEAD, PacketData},
        packet_type::PacketType,
    },
};

pub struct PairingCoordinator {
    pending: Watch<CriticalSectionRawMutex, Option<PendingPairing>, 1>,
    events: Watch<CriticalSectionRawMutex, Option<PairingEvent>, 1>,
}

impl PairingCoordinator {
    pub const fn new() -> Self {
        Self {
            pending: Watch::new_with(None),
            events: Watch::new_with(None),
        }
    }

    pub fn begin(&self, peer_id: [u8; 16], key: [u8; KEY_SIZE]) {
        self.pending.sender().send(Some(PendingPairing {
            peer_id,
            key,
            request_nonce: None,
            response_nonce: None,
        }));
    }

    pub fn pending(&self) -> Option<PendingPairing> {
        self.pending.sender().try_get().unwrap_or_default()
    }

    pub fn set_request_nonce(&self, peer_id: [u8; 16], nonce: [u8; crypto::NONCE_SIZE]) {
        if let Some(mut pending) = self.pending() {
            if pending.peer_id == peer_id {
                pending.request_nonce = Some(nonce);
                self.pending.sender().send(Some(pending));
            }
        }
    }

    pub fn set_response_nonce(&self, peer_id: [u8; 16], nonce: [u8; crypto::NONCE_SIZE]) {
        if let Some(mut pending) = self.pending() {
            if pending.peer_id == peer_id {
                pending.response_nonce = Some(nonce);
                self.pending.sender().send(Some(pending));
            }
        }
    }

    pub fn finish(&self, peer_id: [u8; 16], succeeded: bool, key: [u8; KEY_SIZE]) {
        self.pending.sender().send(None);
        self.events.sender().send(Some(PairingEvent {
            peer_id,
            succeeded,
            key,
        }));
    }

    pub fn event_receiver(
        &self,
    ) -> Option<Receiver<'_, CriticalSectionRawMutex, Option<PairingEvent>, 1>> {
        self.events.receiver()
    }
}

impl Default for PairingCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub fn pairing_stage(packet: &PacketData) -> Option<PairingStage> {
    match packet.packet_type {
        PacketType::PairingConfirm if !packet.payload.is_empty() => match packet.payload[0] {
            0 => Some(PairingStage::Request),
            1 => Some(PairingStage::Challenge),
            2 => Some(PairingStage::Finish),
            _ => None,
        },
        _ => None,
    }
}

pub fn build_request_packet(
    key: &[u8; KEY_SIZE],
    packet_id: u32,
    from: [u8; 16],
    to: [u8; 16],
) -> Option<(PacketData, [u8; crypto::NONCE_SIZE])> {
    build_pairing_packet(key, packet_id, from, to, PairingStage::Request, None, None)
}

pub fn build_challenge_packet(
    key: &[u8; KEY_SIZE],
    packet_id: u32,
    from: [u8; 16],
    to: [u8; 16],
    request_nonce: [u8; crypto::NONCE_SIZE],
) -> Option<(PacketData, [u8; crypto::NONCE_SIZE])> {
    build_pairing_packet(
        key,
        packet_id,
        from,
        to,
        PairingStage::Challenge,
        Some(request_nonce),
        None,
    )
}

pub fn build_finish_packet(
    key: &[u8; KEY_SIZE],
    packet_id: u32,
    from: [u8; 16],
    to: [u8; 16],
    request_nonce: [u8; crypto::NONCE_SIZE],
    response_nonce: [u8; crypto::NONCE_SIZE],
) -> Option<PacketData> {
    build_pairing_packet(
        key,
        packet_id,
        from,
        to,
        PairingStage::Finish,
        Some(request_nonce),
        Some(response_nonce),
    )
    .map(|(packet, _)| packet)
}

/// Largest AAD a pairing frame authenticates: header, stage, and both nonces.
const PAIRING_AAD_SIZE: usize =
    crate::packet::packet_data::AUTHENTICATED_HEADER_SIZE + 1 + 2 * crypto::NONCE_SIZE;

/// Bytes a pairing payload needs: stage byte, exchanged nonces, AEAD nonce and tag.
fn pairing_payload_len(
    request_nonce: Option<[u8; crypto::NONCE_SIZE]>,
    response_nonce: Option<[u8; crypto::NONCE_SIZE]>,
) -> usize {
    let nonce_bytes = request_nonce.is_some() as usize * crypto::NONCE_SIZE
        + response_nonce.is_some() as usize * crypto::NONCE_SIZE;
    1 + nonce_bytes + GCM_OVERHEAD
}

/// Authenticated context of one pairing frame: immutable header, stage, nonces.
///
/// Both the builder and the verifier use this, so the tag always covers the
/// same bytes on both sides.
fn pairing_aad(
    packet: &PacketData,
    stage: PairingStage,
    request_nonce: Option<[u8; crypto::NONCE_SIZE]>,
    response_nonce: Option<[u8; crypto::NONCE_SIZE]>,
) -> ([u8; PAIRING_AAD_SIZE], usize) {
    let mut aad = [0; PAIRING_AAD_SIZE];
    let header = packet.authenticated_header();
    aad[..header.len()].copy_from_slice(&header);
    let mut len = header.len();
    aad[len] = stage as u8;
    len += 1;
    for nonce in [request_nonce, response_nonce].into_iter().flatten() {
        aad[len..len + crypto::NONCE_SIZE].copy_from_slice(&nonce);
        len += crypto::NONCE_SIZE;
    }
    (aad, len)
}

fn build_pairing_packet(
    key: &[u8; KEY_SIZE],
    packet_id: u32,
    from: [u8; 16],
    to: [u8; 16],
    stage: PairingStage,
    request_nonce: Option<[u8; crypto::NONCE_SIZE]>,
    response_nonce: Option<[u8; crypto::NONCE_SIZE]>,
) -> Option<(PacketData, [u8; crypto::NONCE_SIZE])> {
    let payload_len = pairing_payload_len(request_nonce, response_nonce);
    let placeholder = [0; 1 + 2 * crypto::NONCE_SIZE + GCM_OVERHEAD];
    let mut packet = PacketData::new(
        packet_id,
        packet_id,
        0,
        1,
        from,
        to,
        true,
        PacketType::PairingConfirm,
        &placeholder[..payload_len],
        DEFAULT_HOP_LIMIT,
        DEFAULT_HOP_LIMIT,
    )
    .ok()?;

    let (aad, aad_len) = pairing_aad(&packet, stage, request_nonce, response_nonce);
    let (nonce, ciphertext, tag) = crypto::encrypt(key, &[], &aad[..aad_len]).ok()?;
    if !ciphertext.is_empty() {
        return None;
    }

    packet.payload.clear();
    packet.payload.push(stage as u8).ok()?;
    if let Some(request_nonce) = request_nonce {
        packet.payload.extend_from_slice(&request_nonce).ok()?;
    }
    if let Some(response_nonce) = response_nonce {
        packet.payload.extend_from_slice(&response_nonce).ok()?;
    }
    packet.payload.extend_from_slice(&nonce).ok()?;
    packet.payload.extend_from_slice(&tag).ok()?;
    Some((packet, nonce))
}

pub fn verify_pairing_packet(
    packet: &PacketData,
    key: &[u8; KEY_SIZE],
    stage: PairingStage,
    request_nonce: Option<[u8; crypto::NONCE_SIZE]>,
    response_nonce: Option<[u8; crypto::NONCE_SIZE]>,
) -> bool {
    let expected_len = pairing_payload_len(request_nonce, response_nonce);
    if packet.packet_type != PacketType::PairingConfirm
        || !packet.is_encrypted
        || packet.is_broadcast()
        || packet.payload.len() != expected_len
        || pairing_stage(packet) != Some(stage)
    {
        return false;
    }

    let payload = packet.payload.as_slice();
    let mut payload_offset = 1;
    if let Some(request_nonce) = request_nonce {
        if payload[payload_offset..payload_offset + crypto::NONCE_SIZE] != request_nonce {
            return false;
        }
        payload_offset += crypto::NONCE_SIZE;
    }
    if let Some(response_nonce) = response_nonce {
        if payload[payload_offset..payload_offset + crypto::NONCE_SIZE] != response_nonce {
            return false;
        }
        payload_offset += crypto::NONCE_SIZE;
    }

    let nonce_end = payload_offset + crypto::NONCE_SIZE;
    let Ok(nonce) = <[u8; crypto::NONCE_SIZE]>::try_from(&payload[payload_offset..nonce_end])
    else {
        return false;
    };
    let Ok(tag) = <[u8; crypto::TAG_SIZE]>::try_from(&payload[nonce_end..]) else {
        return false;
    };

    let (aad, aad_len) = pairing_aad(packet, stage, request_nonce, response_nonce);
    crypto::decrypt(key, &nonce, &[], &tag, &aad[..aad_len]).is_ok()
}
