use crate::packet::packet_type::PacketType;
use crate::{
    crypto,
    network::{
        discovery::DeviceDiscovery,
        message_delivery::{IncomingMessage, IncomingMessageQueue},
    },
    packet::{
        fragment::Defragmenter,
        packet_data::{MAX_PAYLOAD_SIZE, PacketData},
        packet_deserializer::PacketDeserializer,
    },
    radio::queue::{Packet, QueueError, RadioQueue},
    storage::{contact::Contact, storage_manager::StorageManager},
};
use core::str::from_utf8;
use heapless::Vec;

const SEEN_PACKET_CACHE_CAPACITY: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
struct PacketKey {
    sender: [u8; 16],
    packet_id: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct AcceptedMessage {
    packet_key: PacketKey,
    message_id: u16,
    fingerprint: [u8; 32],
    encryption_key: Option<[u8; crypto::KEY_SIZE]>,
}

pub struct PacketHandler {
    seen_packets: Vec<PacketKey, SEEN_PACKET_CACHE_CAPACITY>,
    accepted_messages: Vec<AcceptedMessage, SEEN_PACKET_CACHE_CAPACITY>,
    defragmenter: Defragmenter,
    next_response_packet_id: u16,
}

impl PacketHandler {
    pub fn new() -> Self {
        Self {
            seen_packets: Vec::new(),
            accepted_messages: Vec::new(),
            defragmenter: Defragmenter::new(),
            next_response_packet_id: 0,
        }
    }

    pub async fn handle_packet(
        &mut self,
        packet: Packet,
        tx_queue: &RadioQueue,
        discovery: &DeviceDiscovery,
        incoming_messages: &IncomingMessageQueue,
        pairing: &super::pairing::PairingCoordinator,
        storage: &mut StorageManager<'static>,
        flash_buffer: &mut [u8],
    ) {
        // Decode packet
        let mut packet: PacketData = match PacketDeserializer::deserialize(packet.as_slice()) {
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

        let broadcast = packet.is_broadcast();
        let addressed_to_this_device = packet.is_for(&device_id);

        // Drop packets already seen in the bounded duplicate cache.
        let packet_key = PacketKey {
            sender: packet.from,
            packet_id: packet.packet_id,
        };
        if !self.remember_packet(packet_key) {
            self.acknowledge_accepted_retry(
                &packet,
                packet_key,
                device_id,
                tx_queue,
                broadcast,
                addressed_to_this_device,
            );
            defmt::debug!(
                "RX duplicate dropped: sender packet id={}",
                packet.packet_id
            );
            return;
        }

        discovery.remember(packet.from);

        if let Some(response) =
            build_announce_response(&packet, device_id, self.next_response_packet_id)
        {
            self.next_response_packet_id = self.next_response_packet_id.wrapping_add(1) & 0x7fff;
            match tx_queue.add_packet(&response) {
                Ok(()) => defmt::debug!("Queued device announce response"),
                Err(QueueError::Full) => {
                    defmt::warn!("Could not respond to announce: TX queue is full")
                }
                Err(QueueError::Packet(_) | QueueError::PacketTooLong { .. }) => {
                    defmt::warn!("Could not build device announce response")
                }
            }
        }

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

        match packet.packet_type {
            PacketType::PairingConfirm if !broadcast && addressed_to_this_device => {
                self.handle_pairing_confirmation(
                    &packet,
                    device_id,
                    tx_queue,
                    pairing,
                    storage,
                    flash_buffer,
                )
                .await;
                return;
            }
            PacketType::Text if broadcast && !packet.is_encrypted => {
                self.handle_broadcast_text(&packet, incoming_messages);
            }
            PacketType::Ack if !broadcast && addressed_to_this_device => {
                deliver_acknowledgement(&packet, storage, flash_buffer, incoming_messages).await;
                return;
            }
            PacketType::Text if !broadcast && addressed_to_this_device => {
                self.handle_direct_text_packet(
                    &packet,
                    device_id,
                    tx_queue,
                    storage,
                    flash_buffer,
                    incoming_messages,
                )
                .await;
                return;
            }
            PacketType::Coordinates if broadcast || addressed_to_this_device => {
                self.handle_coordinates_packet(
                    &packet,
                    device_id,
                    tx_queue,
                    storage,
                    flash_buffer,
                    incoming_messages,
                )
                .await;
                if !broadcast {
                    return;
                }
            }
            _ => {}
        }

        if broadcast {
            forward_packet(&mut packet, tx_queue);
            if packet.is_encrypted {
                return;
            }
            // todo!("Handle broadcast packet");
        }

        if !addressed_to_this_device {
            forward_packet(&mut packet, tx_queue);
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

    fn handle_broadcast_text(
        &mut self,
        packet: &PacketData,
        incoming_messages: &IncomingMessageQueue,
    ) {
        let result = self.defragmenter.push(
            packet.clone(),
            packet.payload.as_slice(),
            packet_fingerprint(packet),
        );
        match result {
            Ok(Some(message)) => {
                deliver_text_payload(
                    incoming_messages,
                    [0; 16],
                    message.first_packet.from,
                    message.payload.as_slice(),
                    false,
                    message.first_packet.message_id,
                );
            }
            Ok(None) => {}
            Err(_) => defmt::warn!("Rejected malformed broadcast text fragments"),
        }
    }

    async fn handle_direct_text_packet(
        &mut self,
        packet: &PacketData,
        device_id: [u8; 16],
        tx_queue: &RadioQueue,
        storage: &mut StorageManager<'static>,
        flash_buffer: &mut [u8],
        incoming_messages: &IncomingMessageQueue,
    ) {
        // Authenticate each radio fragment before storing its plaintext for reassembly.
        let (plaintext, encryption_key) =
            match decode_direct_payload(packet, storage, flash_buffer).await {
                Ok(decoded) => decoded,
                Err(()) => return,
            };
        let result = self.defragmenter.push(
            packet.clone(),
            plaintext.as_slice(),
            packet_fingerprint(packet),
        );
        let message = match result {
            Ok(Some(message)) => message,
            Ok(None) => return,
            Err(_) => {
                defmt::warn!("Rejected malformed direct-message fragments");
                return;
            }
        };

        if !deliver_text_payload(
            incoming_messages,
            packet.from,
            packet.from,
            message.payload.as_slice(),
            message.encrypted,
            message.first_packet.message_id,
        ) {
            return;
        }

        // Cache every fragment only after the complete message has reached the UI queue.
        for fragment in message.accepted_fragments {
            self.remember_accepted_message(AcceptedMessage {
                packet_key: PacketKey {
                    sender: packet.from,
                    packet_id: fragment.packet_id,
                },
                message_id: packet.message_id,
                fingerprint: fragment.fingerprint,
                encryption_key,
            });
        }
        self.queue_ack(&message.first_packet, device_id, encryption_key, tx_queue);
    }

    async fn handle_coordinates_packet(
        &mut self,
        packet: &PacketData,
        device_id: [u8; 16],
        tx_queue: &RadioQueue,
        storage: &mut StorageManager<'static>,
        flash_buffer: &mut [u8],
        incoming_messages: &IncomingMessageQueue,
    ) {
        if packet.fragment_count != 1 {
            defmt::warn!("Rejected fragmented coordinate packet");
            return;
        }
        let broadcast = packet.is_broadcast();
        let (payload, encryption_key) = if broadcast {
            if packet.is_encrypted {
                defmt::warn!("Rejected encrypted broadcast coordinates");
                return;
            }
            let Ok(payload) = heapless::Vec::from_slice(packet.payload.as_slice()) else {
                return;
            };
            (payload, None)
        } else {
            match decode_direct_payload(packet, storage, flash_buffer).await {
                Ok(decoded) => decoded,
                Err(()) => return,
            }
        };
        let Some((latitude_microdegrees, longitude_microdegrees)) =
            decode_coordinates(payload.as_slice())
        else {
            defmt::warn!("Rejected invalid coordinate payload");
            return;
        };

        let accepted = incoming_messages.try_send(IncomingMessage::Coordinates {
            peer_id: if broadcast { [0; 16] } else { packet.from },
            sender_id: packet.from,
            message_id: packet.message_id,
            latitude_microdegrees,
            longitude_microdegrees,
            encrypted: encryption_key.is_some(),
        });
        if accepted.is_err() {
            defmt::warn!("Could not deliver coordinates: conversation queue is full");
            return;
        }

        if !broadcast {
            let packet_key = PacketKey {
                sender: packet.from,
                packet_id: packet.packet_id,
            };
            self.remember_accepted_message(AcceptedMessage {
                packet_key,
                message_id: packet.message_id,
                fingerprint: packet_fingerprint(packet),
                encryption_key,
            });
            self.queue_ack(packet, device_id, encryption_key, tx_queue);
        }
    }

    fn remember_accepted_message(&mut self, accepted: AcceptedMessage) {
        if let Some(index) = self
            .accepted_messages
            .iter()
            .position(|known| known.packet_key == accepted.packet_key)
        {
            self.accepted_messages.remove(index);
        }
        if self.accepted_messages.is_full() {
            self.accepted_messages.remove(0);
        }
        let _ = self.accepted_messages.push(accepted);
    }

    fn acknowledge_accepted_retry(
        &mut self,
        packet: &PacketData,
        packet_key: PacketKey,
        device_id: [u8; 16],
        tx_queue: &RadioQueue,
        broadcast: bool,
        addressed_to_this_device: bool,
    ) {
        if !matches!(
            packet.packet_type,
            PacketType::Text | PacketType::Coordinates
        ) || broadcast
            || !addressed_to_this_device
        {
            return;
        }
        let fingerprint = packet_fingerprint(packet);
        if let Some(accepted) = self
            .accepted_messages
            .iter()
            .find(|accepted| {
                accepted.packet_key == packet_key
                    && accepted.message_id == packet.message_id
                    && accepted.fingerprint == fingerprint
            })
            .copied()
        {
            self.queue_ack(packet, device_id, accepted.encryption_key, tx_queue);
        }
    }

    fn queue_ack(
        &mut self,
        received: &PacketData,
        device_id: [u8; 16],
        encryption_key: Option<[u8; crypto::KEY_SIZE]>,
        tx_queue: &RadioQueue,
    ) {
        let packet_id = self.next_response_packet_id;
        self.next_response_packet_id = self.next_response_packet_id.wrapping_add(1) & 0x7fff;
        let message_id_payload = received.message_id.to_be_bytes();
        let encrypted_payload = [0; crate::packet::packet_data::GCM_OVERHEAD];
        let payload = if encryption_key.is_some() {
            encrypted_payload.as_slice()
        } else {
            message_id_payload.as_slice()
        };
        let mut ack = match PacketData::new(
            packet_id,
            packet_id,
            0,
            1,
            device_id,
            received.from,
            encryption_key.is_some(),
            PacketType::Ack,
            payload,
            crate::packet::packet_data::DEFAULT_HOP_LIMIT,
            crate::packet::packet_data::DEFAULT_HOP_LIMIT,
        ) {
            Ok(ack) => ack,
            Err(_) => {
                defmt::warn!("Could not construct message acknowledgement");
                return;
            }
        };

        if let Some(key) = encryption_key {
            if crypto::encrypt_packet_payload(&mut ack, &key, &message_id_payload).is_err() {
                defmt::warn!("Could not authenticate message acknowledgement");
                return;
            }
        }

        match tx_queue.add_packet(&ack) {
            Ok(()) => defmt::debug!("Queued ACK for message {}", received.message_id),
            Err(QueueError::Full) => defmt::warn!("Could not queue message ACK: TX queue is full"),
            Err(QueueError::Packet(_) | QueueError::PacketTooLong { .. }) => {
                defmt::warn!("Could not queue invalid message ACK")
            }
        }
    }

    async fn handle_pairing_confirmation(
        &mut self,
        packet: &PacketData,
        device_id: [u8; 16],
        tx_queue: &RadioQueue,
        pairing: &super::pairing::PairingCoordinator,
        storage: &mut StorageManager<'static>,
        flash_buffer: &mut [u8],
    ) {
        let Some(pending) = pairing.pending() else {
            return;
        };
        if pending.peer_id != packet.from || device_id == packet.from {
            return;
        }
        use super::pairing::{PairingStage, pairing_stage, verify_pairing_packet};

        match (device_id > packet.from, pairing_stage(packet)) {
            (true, Some(PairingStage::Request)) => {
                if !verify_pairing_packet(packet, &pending.key, PairingStage::Request, None, None) {
                    pairing.finish(packet.from, false, pending.key);
                    return;
                }
                let Some(request_nonce) = packet_nonce(packet, 1) else {
                    pairing.finish(packet.from, false, pending.key);
                    return;
                };
                let response = super::pairing::build_challenge_packet(
                    &pending.key,
                    self.next_response_packet_id,
                    device_id,
                    packet.from,
                    request_nonce,
                );
                self.next_response_packet_id =
                    self.next_response_packet_id.wrapping_add(1) & 0x7fff;
                let Some((response, response_nonce)) = response else {
                    pairing.finish(packet.from, false, pending.key);
                    return;
                };
                pairing.set_request_nonce(packet.from, request_nonce);
                pairing.set_response_nonce(packet.from, response_nonce);
                if tx_queue.add_packet(&response).is_err() {
                    pairing.finish(packet.from, false, pending.key);
                }
                return;
            }
            (false, Some(PairingStage::Challenge)) => {
                let Some(request_nonce) = pending.request_nonce else {
                    return;
                };
                if !verify_pairing_packet(
                    packet,
                    &pending.key,
                    PairingStage::Challenge,
                    Some(request_nonce),
                    None,
                ) {
                    pairing.finish(packet.from, false, pending.key);
                    return;
                }
                let Some(response_nonce) = packet_nonce(packet, 1 + crypto::NONCE_SIZE) else {
                    pairing.finish(packet.from, false, pending.key);
                    return;
                };
                let Some(response) = super::pairing::build_finish_packet(
                    &pending.key,
                    self.next_response_packet_id,
                    device_id,
                    packet.from,
                    request_nonce,
                    response_nonce,
                ) else {
                    pairing.finish(packet.from, false, pending.key);
                    return;
                };
                self.next_response_packet_id =
                    self.next_response_packet_id.wrapping_add(1) & 0x7fff;
                if tx_queue.add_packet(&response).is_err() {
                    pairing.finish(packet.from, false, pending.key);
                    return;
                }
            }
            (true, Some(PairingStage::Finish)) => {
                let (Some(request_nonce), Some(response_nonce)) =
                    (pending.request_nonce, pending.response_nonce)
                else {
                    return;
                };
                if !verify_pairing_packet(
                    packet,
                    &pending.key,
                    PairingStage::Finish,
                    Some(request_nonce),
                    Some(response_nonce),
                ) {
                    pairing.finish(packet.from, false, pending.key);
                    return;
                }
            }
            _ => return,
        }

        let contact = Contact {
            device_id: packet.from,
            key: pending.key,
            custom_name: heapless::String::new(),
        };
        if storage.save_contact(&contact, flash_buffer).await.is_ok() {
            pairing.finish(packet.from, true, pending.key);
        } else {
            pairing.finish(packet.from, false, pending.key);
        }
    }
}

fn packet_nonce(packet: &PacketData, offset: usize) -> Option<[u8; crypto::NONCE_SIZE]> {
    packet
        .payload
        .get(offset..offset + crypto::NONCE_SIZE)?
        .try_into()
        .ok()
}

fn build_announce_response(
    announce: &PacketData,
    device_id: [u8; 16],
    packet_id: u16,
) -> Option<PacketData> {
    match (
        announce.packet_type,
        announce.is_broadcast(),
        announce.is_encrypted,
        announce.payload.is_empty(),
        announce.from == [0; 16],
    ) {
        (PacketType::DeviceAnnounce, true, false, true, false) => {}
        _ => return None,
    }

    PacketData::new(
        packet_id,
        packet_id,
        0,
        1,
        device_id,
        announce.from,
        false,
        PacketType::DeviceAnnounceResponse,
        &[],
        crate::packet::packet_data::DEFAULT_HOP_LIMIT,
        crate::packet::packet_data::DEFAULT_HOP_LIMIT,
    )
    .ok()
}

async fn decode_direct_payload(
    packet: &PacketData,
    storage: &mut StorageManager<'static>,
    flash_buffer: &mut [u8],
) -> Result<
    (
        heapless::Vec<u8, MAX_PAYLOAD_SIZE>,
        Option<[u8; crypto::KEY_SIZE]>,
    ),
    (),
> {
    let (plaintext, key) = match storage.load_contact(&packet.from, flash_buffer).await {
        Ok(Some(contact)) if packet.is_encrypted => {
            match crypto::decrypt_packet_payload(packet, &contact.key) {
                Ok(plaintext) => (plaintext, Some(contact.key)),
                Err(_) => {
                    defmt::warn!("Rejected encrypted packet: authentication failed");
                    return Err(());
                }
            }
        }
        Ok(Some(_)) => {
            defmt::warn!("Rejected plaintext packet for paired contact");
            return Err(());
        }
        Ok(None) if packet.is_encrypted => {
            defmt::warn!("Rejected encrypted packet: contact key is missing");
            return Err(());
        }
        Ok(None) => (packet.payload.as_slice().to_vec(), None),
        Err(_) => {
            defmt::warn!("Rejected packet: contact lookup failed");
            return Err(());
        }
    };
    let mut bounded_payload = heapless::Vec::new();
    bounded_payload
        .extend_from_slice(&plaintext)
        .map_err(|_| ())?;
    Ok((bounded_payload, key))
}

fn decode_coordinates(payload: &[u8]) -> Option<(i32, i32)> {
    if payload.len() != 8 {
        return None;
    }
    let latitude = i32::from_be_bytes(payload[..4].try_into().ok()?);
    let longitude = i32::from_be_bytes(payload[4..].try_into().ok()?);
    // Reject out-of-range WGS84 values before exposing a received location to the UI.
    if !(-90_000_000..=90_000_000).contains(&latitude)
        || !(-180_000_000..=180_000_000).contains(&longitude)
    {
        return None;
    }
    Some((latitude, longitude))
}

fn deliver_text_payload(
    incoming_messages: &IncomingMessageQueue,
    peer_id: [u8; 16],
    sender_id: [u8; 16],
    payload: &[u8],
    encrypted: bool,
    message_id: u16,
) -> bool {
    let text = match from_utf8(payload) {
        Ok(text) => text,
        Err(_) => {
            defmt::warn!("Rejected text message: payload is not UTF-8");
            return false;
        }
    };
    let mut message = heapless::String::<{ super::message_delivery::MESSAGE_TEXT_CAPACITY }>::new();
    if message.push_str(text).is_err() {
        defmt::warn!("Rejected text message: payload exceeds UI capacity");
        return false;
    }
    match incoming_messages.try_send(IncomingMessage::Text {
        peer_id,
        sender_id,
        message_id,
        text: message,
        encrypted,
    }) {
        Ok(()) => {
            defmt::debug!("Delivered received text to conversation UI");
            true
        }
        Err(_) => {
            defmt::warn!("Could not deliver text: conversation queue is full");
            false
        }
    }
}

async fn deliver_acknowledgement(
    packet: &PacketData,
    storage: &mut StorageManager<'static>,
    flash_buffer: &mut [u8],
    incoming_messages: &IncomingMessageQueue,
) {
    let message_id = match storage.load_contact(&packet.from, flash_buffer).await {
        Ok(Some(contact)) if packet.is_encrypted => {
            match crypto::decrypt_packet_payload(packet, &contact.key) {
                Ok(payload) => acknowledgement_message_id(payload.as_slice()),
                Err(_) => {
                    defmt::warn!("Rejected ACK: authentication failed");
                    return;
                }
            }
        }
        Ok(Some(_)) => {
            defmt::warn!("Rejected plaintext ACK for paired contact");
            return;
        }
        Ok(None) if packet.is_encrypted => {
            defmt::warn!("Rejected encrypted ACK: contact key is missing");
            return;
        }
        Ok(None) => acknowledgement_message_id(packet.payload.as_slice()),
        Err(_) => {
            defmt::warn!("Rejected ACK: contact lookup failed");
            return;
        }
    };

    let Some(message_id) = message_id else {
        defmt::warn!("Rejected ACK: payload must contain exactly two bytes");
        return;
    };

    match incoming_messages.try_send(IncomingMessage::Acknowledgement {
        peer_id: packet.from,
        message_id,
    }) {
        Ok(()) => defmt::debug!("Received ACK for message {}", message_id),
        Err(_) => defmt::warn!("Could not deliver ACK to UI: conversation queue is full"),
    }
}

fn acknowledgement_message_id(payload: &[u8]) -> Option<u16> {
    if payload.len() != 2 {
        return None;
    }
    Some(u16::from_be_bytes([payload[0], payload[1]]))
}

fn packet_fingerprint(packet: &PacketData) -> [u8; 32] {
    let header = packet.authenticated_header();
    let payload = packet.payload.as_slice();
    let mut bytes = [0; crate::packet::packet_data::AUTHENTICATED_HEADER_SIZE
        + crate::packet::packet_data::MAX_PAYLOAD_SIZE];
    let header_len = header.len();
    bytes[..header_len].copy_from_slice(&header);
    bytes[header_len..header_len + payload.len()].copy_from_slice(payload);
    crypto::hash(&bytes[..header_len + payload.len()])
}

// Decrement a packet's remaining hops before queueing it for forwarding.
fn forward_packet(packet: &mut PacketData, tx_queue: &RadioQueue) {
    if !packet.decrement_hop_limit() {
        defmt::debug!("Packet reached its hop limit; not forwarding");
        return;
    }

    match tx_queue.add_packet(packet) {
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
