//! Receive path for radio frames: validate, deduplicate, dispatch, acknowledge, relay.
//! Design notes: `docs/packet-handler.md`; message delivery: `docs/messaging.md`.

use core::str::from_utf8;

use heapless::Vec;

use crate::storage::message::ConversationId;
use crate::{
    DEVICE_ID, crypto,
    network::{
        discovery::DeviceDiscovery,
        outbox,
        pairing::{
            PairingCoordinator, build_challenge_packet, build_finish_packet, pairing_stage,
            verify_pairing_packet,
        },
        types::PairingStage,
    },
    notify,
    packet::{
        fragment::{Defragmenter, ReassembledMessage},
        packet_data::{DEFAULT_HOP_LIMIT, GCM_OVERHEAD, MAX_PAYLOAD_SIZE, PacketData},
        packet_deserializer::PacketDeserializer,
        packet_type::PacketType,
    },
    radio::queue::{Packet, QueueError, RadioQueue},
    storage::{
        contact::Contact,
        message::{MessageType, StoredMessage},
        session::Flash,
    },
};

/// Fragment keys remembered for duplicate suppression.
const SEEN_CAPACITY: usize = 128;
/// Accepted fragments remembered for a repeated acknowledgement.
const ACCEPTED_CAPACITY: usize = 128;
/// Acknowledgement payload: original sender, recipient, and message ID.
const ACK_PAYLOAD_SIZE: usize = 16 + 16 + 4;

/// Plaintext of one received fragment.
type Payload = Vec<u8, MAX_PAYLOAD_SIZE>;
/// Key of one paired contact.
type PairKey = [u8; crypto::KEY_SIZE];

/// One authenticated fragment, kept so a retransmit still gets its ACK.
struct Accepted {
    sender_id: [u8; 16],
    packet_id: u32,
    message_id: u32,
    fingerprint: [u8; 32],
    key: PairKey,
}

/// Verifies frames, rebuilds logical messages, and answers their senders.
pub struct PacketHandler {
    /// `(sender, packet_id)` of every fragment already processed.
    seen: Vec<([u8; 16], u32), SEEN_CAPACITY>,
    /// Fragments of messages that were authenticated and delivered.
    accepted: Vec<Accepted, ACCEPTED_CAPACITY>,
    /// Rebuilds one logical message from its fragments.
    fragments: Defragmenter,
}

impl PacketHandler {
    pub fn new() -> Self {
        Self {
            seen: Vec::new(),
            accepted: Vec::new(),
            fragments: Defragmenter::new(),
        }
    }

    /// Answer one RX frame; `None` stops all further dispatch.
    pub async fn handle_packet(
        &mut self,
        frame: Packet,
        tx: &RadioQueue,
        discovery: &DeviceDiscovery,
        pairing: &PairingCoordinator,
        flash: &Flash,
    ) {
        // Drop frames that do not parse.
        let Some(mut packet) = parse_frame(&frame) else {
            return;
        };
        // Refuse traffic while this device has no identity.
        let Some(device_id) = DEVICE_ID.try_get().flatten() else {
            defmt::warn!("RX ignored: this device has no stored identity");
            return;
        };
        // Drop packets this device sent and a relay echoed back.
        if packet.from == device_id {
            defmt::debug!("RX ignored: packet originated from this device");
            return;
        }

        // Classify the destination once for every branch below.
        let broadcast = packet.is_broadcast();
        let here = packet.is_for(&device_id);

        // Answer a retransmit from the accepted cache and stop.
        if self.is_duplicate(packet.from, packet.packet_id) {
            self.repeat_ack(&packet, device_id, broadcast, here, tx, flash)
                .await;
            defmt::debug!("RX duplicate dropped: id={}", packet.packet_id);
            return;
        }

        // Record the sender so the UI can offer it; an announcement also
        // feeds the active search list.
        if packet.packet_type == PacketType::DeviceAnnounce {
            discovery.remember_announce(packet.from);
        } else {
            discovery.remember(packet.from);
        }
        // Log one line per accepted frame.
        log_frame(&packet, broadcast, here);

        // Dispatch by type; consumed traffic stops here.
        match packet.packet_type {
            PacketType::PairingConfirm if !broadcast && here => {
                self.advance_pairing(&packet, device_id, tx, pairing, flash)
                    .await;
                return;
            }
            PacketType::Ack if !broadcast && here => {
                self.accept_ack(&packet, flash).await;
                return;
            }
            PacketType::Text if broadcast && !packet.is_encrypted => {
                self.accept_broadcast_text(&packet, flash).await;
            }
            PacketType::Text if !broadcast && here => {
                self.accept_direct_text(&packet, device_id, tx, flash).await;
                return;
            }
            PacketType::Coordinates if broadcast || here => {
                self.accept_coordinates(&packet, device_id, tx, flash).await;
                if !broadcast {
                    return;
                }
            }
            // A direct add-contact request only prompts the user.
            PacketType::DeviceAnnounceResponse if !broadcast && here => {
                Self::accept_contact_request(&packet, flash).await;
                return;
            }
            // Relay everything else further down.
            _ => {}
        }

        // Relay broadcasts and traffic addressed to another device.
        if broadcast || !here {
            forward(&mut packet, tx);
        }
    }

    /// Record one fragment key; true when it was already seen.
    fn is_duplicate(&mut self, sender_id: [u8; 16], packet_id: u32) -> bool {
        // Report a repeated key so the caller can drop the duplicate.
        let key = (sender_id, packet_id);
        if self.seen.contains(&key) {
            return true;
        }
        // Evict the oldest key to keep one slot free.
        if self.seen.is_full() {
            self.seen.remove(0);
        }
        let _ = self.seen.push(key);
        false
    }

    /// Repeat the ACK for a fragment that was already accepted and delivered.
    async fn repeat_ack(
        &mut self,
        packet: &PacketData,
        device_id: [u8; 16],
        broadcast: bool,
        here: bool,
        tx: &RadioQueue,
        flash: &Flash,
    ) {
        // Repeat only for direct text and coordinates.
        if broadcast
            || !here
            || !matches!(
                packet.packet_type,
                PacketType::Text | PacketType::Coordinates
            )
        {
            return;
        }

        // Match sender, packet, message, and content so a reused ID stays silent.
        let content = fingerprint(packet);
        let Some(key) = self
            .accepted
            .iter()
            .find(|accepted| {
                accepted.sender_id == packet.from
                    && accepted.packet_id == packet.packet_id
                    && accepted.message_id == packet.message_id
                    && accepted.fingerprint == content
            })
            .map(|accepted| accepted.key)
        else {
            return;
        };

        // Repeat the ACK with the key that accepted the original fragment.
        self.send_ack(packet, device_id, key, tx, flash).await;
    }

    /// Deliver one complete plaintext broadcast message.
    async fn accept_broadcast_text(&mut self, packet: &PacketData, flash: &Flash) {
        // Rebuild the logical message from its fragments.
        let Some(message) = self.reassemble(packet, packet.payload.as_slice(), "broadcast text")
        else {
            return;
        };
        // Deliver it to the public conversation.
        deliver_text(ConversationId::Broadcast, &message, flash).await;
    }

    /// Deliver one direct message, then acknowledge it when authenticated.
    async fn accept_direct_text(
        &mut self,
        packet: &PacketData,
        device_id: [u8; 16],
        tx: &RadioQueue,
        flash: &Flash,
    ) {
        // Apply the pair-key rules before reassembly.
        let Some((plaintext, pair_key)) = decrypt_direct(packet, flash).await else {
            return;
        };
        // Rebuild the logical message from its decrypted fragments.
        let Some(message) = self.reassemble(packet, plaintext.as_slice(), "direct text") else {
            return;
        };

        // Deliver once; skip the ACK when storage refuses the text.
        let conversation = ConversationId::Direct(packet.from);
        if deliver_text(conversation, &message, flash).await == SaveResult::Failed {
            return;
        }
        // Acknowledge only authenticated traffic.
        let Some(key) = pair_key else {
            return;
        };

        // Cache every fragment so a retransmit can be acknowledged again.
        let message_id = message.first_packet.message_id;
        for fragment in message.accepted_fragments.iter() {
            self.remember_accepted(Accepted {
                sender_id: packet.from,
                packet_id: fragment.packet_id,
                message_id,
                fingerprint: fragment.fingerprint,
                key,
            });
        }
        // Tell the sender the whole message arrived.
        self.send_ack(&message.first_packet, device_id, key, tx, flash)
            .await;
    }

    /// Deliver one position row, then acknowledge it when authenticated.
    async fn accept_coordinates(
        &mut self,
        packet: &PacketData,
        device_id: [u8; 16],
        tx: &RadioQueue,
        flash: &Flash,
    ) {
        // Reject a coordinate message split across fragments.
        if packet.fragment_count != 1 {
            defmt::warn!("Rejected fragmented coordinate packet");
            return;
        }

        // Decode a broadcast payload as-is, or authenticate a direct one.
        let broadcast = packet.is_broadcast();
        let (payload, pair_key) = if broadcast {
            // Reject broadcast coordinates that claim to be encrypted.
            if packet.is_encrypted {
                defmt::warn!("Rejected encrypted broadcast coordinates");
                return;
            }
            let Ok(payload) = Payload::from_slice(packet.payload.as_slice()) else {
                return;
            };
            (payload, None)
        } else {
            let Some(decoded) = decrypt_direct(packet, flash).await else {
                return;
            };
            decoded
        };

        // Reject a payload that is not two WGS84 coordinates.
        let Some((latitude, longitude)) = decode_coordinates(payload.as_slice()) else {
            defmt::warn!("Rejected invalid coordinate payload");
            return;
        };

        // Save the row to its conversation and tell the UI it arrived.
        let conversation = ConversationId::from_peer(packet.from);
        let stored = StoredMessage::new(
            packet.message_id,
            false,
            crate::now_timestamp(),
            MessageType::Coords {
                latitude_microdegrees: latitude,
                longitude_microdegrees: longitude,
            },
            "",
            packet.packet_id,
            packet.message_id,
            packet.from,
            pair_key.is_some(),
        );
        if save_message(conversation, &stored, flash).await == SaveResult::Failed {
            return;
        }

        // Answer only an authenticated row.
        let Some(key) = pair_key else {
            return;
        };
        self.remember_accepted(Accepted {
            sender_id: packet.from,
            packet_id: packet.packet_id,
            message_id: packet.message_id,
            fingerprint: fingerprint(packet),
            key,
        });
        self.send_ack(packet, device_id, key, tx, flash).await;
    }

    /// Authenticate an acknowledgement and mark the sent message delivered.
    async fn accept_ack(&mut self, packet: &PacketData, flash: &Flash) {
        // Decrypt with the sender's pair key; a plaintext ACK proves nothing.
        let Some((payload, pair_key)) = decrypt_direct(packet, flash).await else {
            return;
        };
        if pair_key.is_none() {
            defmt::warn!("Rejected unauthenticated ACK");
            return;
        }

        // Require the payload to name this exchange: the stored sent message's
        // sender is this device, and the ACK sender is the peer.
        let Some(message_id) = decode_ack(payload.as_slice(), &packet.to, &packet.from) else {
            defmt::warn!("Rejected ACK: payload names another exchange");
            return;
        };

        // Stop retransmitting: the peer has the whole message.
        outbox::acknowledge(message_id);

        // Flip our records to the delivered state and tell the UI.
        let marked = flash
            .with(async |session| {
                session
                    .storage
                    .mark_acknowledged(&packet.from, message_id, &mut session.buffer)
                    .await
            })
            .await;
        match marked {
            Ok(0) => defmt::warn!("Received ACK for a message that is not stored"),
            Ok(_) => notify::history_changed(),
            Err(_) => defmt::warn!("Could not mark the acknowledged message in flash"),
        }
    }

    /// Show an add-contact request from a peer; nothing is stored or keyed.
    async fn accept_contact_request(packet: &PacketData, flash: &Flash) {
        let looked_up = flash
            .with(async |session| {
                session
                    .storage
                    .load_contact(&packet.from, &mut session.buffer)
                    .await
            })
            .await;
        // A blocked peer's request is dropped like any other traffic of theirs.
        if matches!(looked_up, Ok(Some(contact)) if contact.blocked) {
            defmt::debug!("Dropped contact request from a blocked peer");
            return;
        }
        defmt::info!("Heard an add-contact request");
        notify::contact_request(packet.from);
    }

    /// Advance the pending handshake by one stage, then save the pair key.
    async fn advance_pairing(
        &mut self,
        packet: &PacketData,
        device_id: [u8; 16],
        tx: &RadioQueue,
        pairing: &PairingCoordinator,
        flash: &Flash,
    ) {
        // Ignore traffic that does not belong to the pending handshake.
        let Some(pending) = pairing.pending() else {
            return;
        };
        if pending.peer_id != packet.from {
            return;
        }
        let key = pending.key;
        let fail = || pairing.finish(packet.from, false, key);

        match (device_id > packet.from, pairing_stage(packet)) {
            // Answer a verified request with a challenge and keep waiting.
            (true, Some(PairingStage::Request)) => {
                if !verify_pairing_packet(packet, &key, PairingStage::Request, None, None) {
                    fail();
                    return;
                }
                let Some(request_nonce) = read_nonce(packet, 1) else {
                    fail();
                    return;
                };
                let request_id = flash.reserve_packet_ids(1).await;
                let Some((response, response_nonce)) =
                    build_challenge_packet(&key, request_id, device_id, packet.from, request_nonce)
                else {
                    fail();
                    return;
                };
                pairing.set_request_nonce(packet.from, request_nonce);
                pairing.set_response_nonce(packet.from, response_nonce);
                if tx.add_packet(&response).is_err() {
                    fail();
                }
                return;
            }
            // Answer a verified challenge with the closing packet.
            (false, Some(PairingStage::Challenge)) => {
                let Some(request_nonce) = pending.request_nonce else {
                    return;
                };
                if !verify_pairing_packet(
                    packet,
                    &key,
                    PairingStage::Challenge,
                    Some(request_nonce),
                    None,
                ) {
                    fail();
                    return;
                }
                let Some(response_nonce) = read_nonce(packet, 1 + crypto::NONCE_SIZE) else {
                    fail();
                    return;
                };
                let response_id = flash.reserve_packet_ids(1).await;
                let Some(response) = build_finish_packet(
                    &key,
                    response_id,
                    device_id,
                    packet.from,
                    request_nonce,
                    response_nonce,
                ) else {
                    fail();
                    return;
                };
                if tx.add_packet(&response).is_err() {
                    fail();
                    return;
                }
            }
            // Verify the closing packet that proves both sides hold the key.
            (true, Some(PairingStage::Finish)) => {
                let (Some(request_nonce), Some(response_nonce)) =
                    (pending.request_nonce, pending.response_nonce)
                else {
                    return;
                };
                if !verify_pairing_packet(
                    packet,
                    &key,
                    PairingStage::Finish,
                    Some(request_nonce),
                    Some(response_nonce),
                ) {
                    fail();
                    return;
                }
            }
            // Ignore a stage this side is not waiting for.
            _ => return,
        }

        // Store the pair key so later direct traffic can be authenticated.
        let contact = Contact {
            device_id: packet.from,
            key,
            custom_name: heapless::String::new(),
            blocked: false,
        };
        let saved = flash
            .with(async |session| {
                session
                    .storage
                    .save_contact(&contact, &mut session.buffer)
                    .await
                    .is_ok()
            })
            .await;
        if saved {
            pairing.finish(packet.from, true, key);
        } else {
            fail();
        }
    }

    /// Feed one fragment to the reassembler; warn when the set conflicts.
    fn reassemble(
        &mut self,
        packet: &PacketData,
        plaintext: &[u8],
        kind: &str,
    ) -> Option<ReassembledMessage> {
        match self
            .fragments
            .push(packet.clone(), plaintext, fingerprint(packet))
        {
            // `Ok(None)` means more fragments are still missing.
            Ok(message) => message,
            Err(_) => {
                defmt::warn!("Rejected malformed {} fragments", kind);
                None
            }
        }
    }

    /// Store one accepted fragment, replacing an older copy of the same fragment.
    fn remember_accepted(&mut self, accepted: Accepted) {
        // Drop any earlier record for this fragment so the newest copy survives.
        if let Some(index) = self.accepted.iter().position(|known| {
            known.sender_id == accepted.sender_id && known.packet_id == accepted.packet_id
        }) {
            self.accepted.remove(index);
        }
        // Evict the oldest record to keep one slot free.
        if self.accepted.is_full() {
            self.accepted.remove(0);
        }
        let _ = self.accepted.push(accepted);
    }

    /// Queue one authenticated acknowledgement for an accepted message.
    async fn send_ack(
        &mut self,
        received: &PacketData,
        device_id: [u8; 16],
        key: PairKey,
        tx: &RadioQueue,
        flash: &Flash,
    ) {
        // Name the original sender, this device, and the logical message.
        let mut acknowledgement = [0; ACK_PAYLOAD_SIZE];
        acknowledgement[..16].copy_from_slice(&received.from);
        acknowledgement[16..32].copy_from_slice(&device_id);
        acknowledgement[32..].copy_from_slice(&received.message_id.to_be_bytes());

        // Reserve the GCM overhead; encryption below replaces the payload.
        let mut ack = match PacketData::new(
            flash.reserve_packet_ids(1).await,
            received.message_id,
            0,
            1,
            device_id,
            received.from,
            true,
            PacketType::Ack,
            &[0; GCM_OVERHEAD],
            DEFAULT_HOP_LIMIT,
            DEFAULT_HOP_LIMIT,
        ) {
            Ok(ack) => ack,
            Err(_) => {
                defmt::warn!("Could not build message acknowledgement");
                return;
            }
        };
        // Authenticate the ACK with the key that protected the message.
        if crypto::encrypt_packet_payload(&mut ack, &key, &acknowledgement).is_err() {
            defmt::warn!("Could not authenticate message acknowledgement");
            return;
        }

        // Queue the ACK; a full queue leaves the sender without delivery state.
        match tx.add_packet(&ack) {
            Ok(()) => {}
            Err(QueueError::Full) => defmt::warn!("Could not queue message ACK: TX queue is full"),
            Err(_) => defmt::warn!("Could not queue invalid message ACK"),
        }
    }
}

impl Default for PacketHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse one raw frame into a packet, rejecting invalid header fields.
fn parse_frame(frame: &Packet) -> Option<PacketData> {
    // Reject a frame that does not match the wire format.
    let packet = match PacketDeserializer::deserialize(frame.as_slice()) {
        Ok(packet) => packet,
        Err(_) => {
            defmt::warn!("RX invalid packet ({} bytes)", frame.len());
            return None;
        }
    };
    // Reject a decoded packet with inconsistent flags, sizes, or hop limits.
    if packet.validate().is_err() {
        defmt::warn!("RX rejected packet with invalid fields");
        return None;
    }
    Some(packet)
}

/// Apply the pair-key rules to a direct packet and return its plaintext.
///
/// The `Option<PairKey>` is `Some` only when the payload was authenticated.
async fn decrypt_direct(packet: &PacketData, flash: &Flash) -> Option<(Payload, Option<PairKey>)> {
    // Look up the key this sender is expected to use.
    let looked_up = flash
        .with(async |session| {
            session
                .storage
                .load_contact(&packet.from, &mut session.buffer)
                .await
        })
        .await;
    let Ok(saved) = looked_up else {
        defmt::warn!("Rejected packet: contact lookup failed");
        return None;
    };
    // A blocked peer is dropped before any message of theirs is processed.
    if saved.as_ref().is_some_and(|contact| contact.blocked) {
        defmt::warn!("Dropped packet from a blocked contact");
        return None;
    }

    match (saved, packet.is_encrypted) {
        // Decrypt traffic from a paired contact and keep its key.
        (Some(contact), true) => match crypto::decrypt_packet_payload(packet, &contact.key) {
            Ok(plaintext) => Payload::from_slice(plaintext.as_slice())
                .ok()
                .map(|payload| (payload, Some(contact.key))),
            Err(_) => {
                defmt::warn!("Rejected encrypted packet: authentication failed");
                None
            }
        },
        // Reject plaintext from a contact that is expected to encrypt.
        (Some(_), false) => {
            defmt::warn!("Rejected plaintext packet for paired contact");
            None
        }
        // Reject encrypted traffic from an unpaired sender that has no key.
        (None, true) => {
            defmt::warn!("Rejected encrypted packet: contact key is missing");
            None
        }
        // Accept a plaintext introduction.
        (None, false) => Payload::from_slice(packet.payload.as_slice())
            .ok()
            .map(|payload| (payload, None)),
    }
}

/// Outcome of one inbound record append.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveResult {
    Stored,
    Duplicate,
    Failed,
}

/// Save one reassembled text message and toast the UI.
async fn deliver_text(
    conversation: ConversationId,
    message: &ReassembledMessage,
    flash: &Flash,
) -> SaveResult {
    // Reject a payload that is not valid UTF-8.
    let Ok(text) = from_utf8(message.payload.as_slice()) else {
        defmt::warn!("Rejected text message: payload is not UTF-8");
        return SaveResult::Failed;
    };
    let first = &message.first_packet;

    // Reject text longer than the record limit before it reaches flash.
    if text.len() > crate::storage::message::MAX_MESSAGE_PAYLOAD {
        defmt::warn!("Rejected text message: it exceeds the storage capacity");
        return SaveResult::Failed;
    }

    let stored = StoredMessage::new(
        first.message_id,
        false,
        crate::now_timestamp(),
        MessageType::Text,
        text,
        first.packet_id,
        first.message_id,
        first.from,
        message.encrypted,
    );
    save_message(conversation, &stored, flash).await
}

/// Append one record to flash and toast the UI only after it persisted.
async fn save_message(
    conversation: ConversationId,
    stored: &StoredMessage,
    flash: &Flash,
) -> SaveResult {
    // A retransmit can arrive after a reboot; the stored record proves the
    // message was already delivered even when the RAM duplicate cache is gone.
    let duplicate = flash
        .with(async |session| {
            session
                .storage
                .recent_message_exists(
                    &conversation.storage_id(),
                    &stored.sender_id,
                    stored.packet_id,
                    stored.message_id,
                    &mut session.buffer,
                )
                .await
        })
        .await;
    match duplicate {
        Ok(true) => return SaveResult::Duplicate,
        Ok(false) => {}
        Err(_) => defmt::warn!("Could not check recent history for a duplicate"),
    }

    let saved = flash
        .with(async |session| {
            session
                .storage
                .append_messages(
                    &conversation.storage_id(),
                    core::slice::from_ref(stored),
                    &mut session.buffer,
                )
                .await
                .is_ok()
        })
        .await;
    if saved {
        // A position row shows its marker; message text is cut to one line.
        let body = match stored.kind.coordinates() {
            Some(_) => "[GPS]",
            None => stored.text.as_str(),
        };
        notify::message_received(conversation, stored.sender_id, body);
        SaveResult::Stored
    } else {
        defmt::warn!("Could not save the received message to flash");
        notify::storage_full(conversation);
        SaveResult::Failed
    }
}

/// Decode two big-endian coordinates and reject values outside the WGS84 range.
fn decode_coordinates(payload: &[u8]) -> Option<(i32, i32)> {
    // Require exactly one latitude and one longitude.
    if payload.len() != 8 {
        return None;
    }
    let latitude = i32::from_be_bytes(payload[..4].try_into().ok()?);
    let longitude = i32::from_be_bytes(payload[4..].try_into().ok()?);

    // Reject out-of-range values before a location reaches the log.
    if !(-90_000_000..=90_000_000).contains(&latitude)
        || !(-180_000_000..=180_000_000).contains(&longitude)
    {
        return None;
    }
    Some((latitude, longitude))
}

/// Read the acknowledged message's sender, recipient, and message ID.
///
/// `named_sender` is the message sender named first in the payload, and
/// `named_recipient` the ACK sender named second. On this device the payload
/// names ourselves first and the peer second, so a forged or misrouted ACK
/// naming another exchange is rejected here.
fn decode_ack(payload: &[u8], named_sender: &[u8; 16], named_recipient: &[u8; 16]) -> Option<u32> {
    // Require exactly the three fields that identify one message.
    if payload.len() != ACK_PAYLOAD_SIZE {
        return None;
    }
    // Reject an ACK that names another conversation or another sender.
    if payload[..16] != *named_sender || payload[16..32] != *named_recipient {
        return None;
    }
    Some(u32::from_be_bytes([
        payload[32],
        payload[33],
        payload[34],
        payload[35],
    ]))
}

/// Read one nonce-sized slice of a pairing payload.
fn read_nonce(packet: &PacketData, offset: usize) -> Option<[u8; crypto::NONCE_SIZE]> {
    packet
        .payload
        .get(offset..offset + crypto::NONCE_SIZE)?
        .try_into()
        .ok()
}

/// Hash the immutable header and payload of one fragment.
fn fingerprint(packet: &PacketData) -> [u8; 32] {
    // Exclude the hop limit so a relayed copy still matches. Hashes the two
    // runs directly to avoid a 254-byte stack copy.
    let header = packet.authenticated_header();
    crypto::hash_parts(&[&header, packet.payload.as_slice()])
}

/// Decrement the hop limit and queue the packet for relaying.
fn forward(packet: &mut PacketData, tx: &RadioQueue) {
    // Stop relaying once the packet has no hops left.
    if !packet.decrement_hop_limit() {
        defmt::debug!("Packet reached its hop limit; not forwarding");
        return;
    }

    // Queue the copy this device relays on behalf of the sender.
    match tx.add_packet(packet) {
        Ok(()) => {}
        Err(QueueError::Full) => defmt::warn!("Could not forward packet: TX queue is full"),
        Err(QueueError::Packet(_)) => defmt::warn!("Could not forward packet: validation failed"),
        Err(QueueError::PacketTooLong { length, maximum }) => {
            defmt::warn!(
                "Could not forward oversized packet: {} bytes (max {})",
                length,
                maximum
            );
        }
    }
}

/// Log one accepted frame together with its routing decisions.
fn log_frame(packet: &PacketData, broadcast: bool, here: bool) {
    defmt::info!(
        "RX from= {} type={} id={} message={} encrypted={} broadcast={} addressed_here={} hops={}/{} bytes={}",
        packet.from,
        packet.packet_type as u8,
        packet.packet_id,
        packet.message_id,
        packet.is_encrypted,
        broadcast,
        here,
        packet.hop_limit,
        packet.initial_hop_limit,
        packet.encoded_len(),
    );
}
