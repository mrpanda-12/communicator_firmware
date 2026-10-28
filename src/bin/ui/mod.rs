//! Application UI state and display rendering.
//!
//! Screen text and chat rows come from flash; the network task only publishes
//! notifications here, so there is never a second copy of chat history in RAM.

pub mod contact;
pub mod display_settings;
#[cfg(feature = "fake-navigator")]
pub mod fake_navigator;
pub mod navigation;
pub mod render;
pub mod state;

use crate::crypto;
use crate::hardware::keypad::KeypadQueue;
use crate::network::{
    pairing::{PairingCoordinator, build_request_packet},
    types::{DiscoveryList, PairingEvent},
};
use crate::notify;
use crate::packet::{
    fragment::Fragmenter,
    packet_data::{DEFAULT_HOP_LIMIT, MAX_FRAGMENT_COUNT, PacketData},
    packet_type::PacketType,
};
use crate::radio::{
    queue::{QueueError, RadioQueue},
    types::RadioConfig,
};
use crate::storage::message::{ConversationId, Direction, MessageKind, StoredMessage};
use crate::storage::session::Flash;
use crate::ui::{
    contact::ContactBook,
    state::{ManageCommand, ManageAction, OutgoingBody, OutgoingMessage},
};
use embassy_futures::select::{Either, select};
use heapless::Vec as HeaplessVec;
use state::{AnnouncementStatus, ChatRow, PairingUiStatus, UiState};
use state::MAX_CHAT_MESSAGES;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpsSnapshot {
    pub fix: bool,
    pub latitude_microdegrees: i32,
    pub longitude_microdegrees: i32,
    pub hour: u8,
    pub minute: u8,
    pub has_time: bool,
    /// UTC calendar date, reported by RMC sentences.
    pub date: Option<crate::hardware::gps::GpsDate>,
}
pub fn gps_snapshot(
    coordinate: Option<crate::hardware::gps::Coordinate>,
    time: Option<crate::hardware::gps::GpsTime>,
) -> GpsSnapshot {
    let (latitude_microdegrees, longitude_microdegrees, fix) = match coordinate {
        Some(coordinate) => (
            (coordinate.latitude_degrees * 1_000_000.0) as i32,
            (coordinate.longitude_degrees * 1_000_000.0) as i32,
            true,
        ),
        None => (0, 0, false),
    };

    let (hour, minute, has_time) = match time {
        Some(time) => (time.hour as u8, time.minute as u8, true),
        None => (0, 0, false),
    };

    GpsSnapshot {
        fix,
        latitude_microdegrees,
        longitude_microdegrees,
        hour,
        minute,
        has_time,
        date: time.and_then(|time| time.date),
    }
}

/// 2/ Render the current UI model with a consistent, always-visible GPS bar.
pub fn draw<D>(display: &mut D, state: &UiState, gps: GpsSnapshot) -> Result<(), D::Error>
where
    D: embedded_graphics::prelude::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
{
    render::draw(display, state, gps)
}

/// 3/ React to screen events, chat storage, and received-message notifications.
pub async fn run<'a, D>(
    mut display: D,
    device_id: Option<[u8; 16]>,
    contacts: ContactBook,
    firmware_version: &'static str,
    timezone_offset_minutes: i32,
    pairing: &PairingCoordinator,
    flash: &'a Flash,
    mut pairing_events: embassy_sync::watch::Receiver<
        'a,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        Option<PairingEvent>,
        1,
    >,
    mut discovery: embassy_sync::watch::Receiver<
        'a,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        DiscoveryList,
        1,
    >,
    radio_queue: &'a RadioQueue,
    mut radio_config: embassy_sync::watch::Receiver<
        'a,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        RadioConfig,
        1,
    >,
    mut gps: embassy_sync::watch::Receiver<
        'static,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        GpsSnapshot,
        2,
    >,
    keypad_events: &'a KeypadQueue,
) -> !
where
    D: embedded_graphics::prelude::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
{
    let initial_radio_config = radio_config.get().await;
    let mut state = UiState::new(
        device_id,
        contacts,
        firmware_version,
        initial_radio_config,
        DiscoveryList::new(),
        timezone_offset_minutes,
    );

    #[cfg(feature = "fake-navigator")]
    let mut fake_navigator = fake_navigator::FakeNavigator::new();
    let mut current_gps = gps.get().await;
    state.update_gps_snapshot(current_gps);

    // Fill every visible chat row preview straight from flash at boot.
    seed_previews(&mut state, flash).await;

    let popups = notify::popup_receiver();
    let mut changes = notify::revision_receiver();
    let mut previous_state = None;
    let mut previous_gps = None;
    let mut next_packet_id = 0x8000u16;
    let mut synced = None;

    loop {
        #[cfg(feature = "fake-navigator")]
        match select(
            select(
                select(
                    select(gps.changed(), radio_config.changed()),
                    select(discovery.changed(), pairing_events.changed()),
                ),
                select(select(popups.receive(), changes.changed()), keypad_events.receive()),
            ),
            fake_navigator.next_key(),
        )
        .await
        {
            Either::First(Either::First(Either::First(Either::First(snapshot)))) => {
                current_gps = snapshot;
                state.update_gps_snapshot(snapshot);
            }
            Either::First(Either::First(Either::First(Either::Second(config)))) => {
                state.radio_config = config
            }
            Either::First(Either::First(Either::Second(Either::First(devices)))) => {
                state.discovered_devices = filter_unsaved_peers(&state.contacts, devices)
            }
            Either::First(Either::First(Either::Second(Either::Second(event)))) => {
                if let Some(event) = event {
                    state.handle_pairing_event(event);
                }
            }
            Either::First(Either::Second(Either::First(Either::First(popup)))) => {
                handle_popup(&mut state, flash, popup).await;
            }
            Either::First(Either::Second(Either::First(Either::Second(_)))) => {}
            Either::First(Either::Second(Either::Second(key))) => {
                state.handle_keypad(key);
            }
            Either::Second(key) => {
                state.handle_key(key);
            }
        }
        #[cfg(not(feature = "fake-navigator"))]
        match select(
            select(
                select(gps.changed(), radio_config.changed()),
                select(discovery.changed(), pairing_events.changed()),
            ),
            select(select(popups.receive(), changes.changed()), keypad_events.receive()),
        )
        .await
        {
            Either::First(Either::First(Either::First(snapshot))) => {
                current_gps = snapshot;
                state.update_gps_snapshot(snapshot);
            }
            Either::First(Either::First(Either::Second(config))) => state.radio_config = config,
            Either::First(Either::Second(Either::First(devices))) => {
                state.discovered_devices = filter_unsaved_peers(&state.contacts, devices)
            }
            Either::First(Either::Second(Either::Second(event))) => {
                if let Some(event) = event {
                    state.handle_pairing_event(event);
                }
            }
            Either::Second(Either::First(Either::First(popup))) => {
                handle_popup(&mut state, flash, popup).await;
            }
            Either::Second(Either::First(Either::Second(_))) => {}
            Either::Second(Either::Second(key)) => state.handle_keypad(key),
        }

        if state.take_pending_announcement() {
            let status = match state.device_id {
                Some(device_id) => {
                    let packet_id = next_packet_id;
                    next_packet_id = next_packet_id.wrapping_add(1);
                    match PacketData::new(
                        packet_id,
                        packet_id,
                        0,
                        1,
                        device_id,
                        [0; 16],
                        false,
                        PacketType::DeviceAnnounce,
                        &[],
                        DEFAULT_HOP_LIMIT,
                        DEFAULT_HOP_LIMIT,
                    ) {
                        Ok(packet) => match radio_queue.add_packet(&packet) {
                            Ok(()) => {
                                defmt::info!("Queued unencrypted device announcement");
                                AnnouncementStatus::Queued
                            }
                            Err(QueueError::Full) => {
                                defmt::warn!("Could not announce device: TX queue is full");
                                AnnouncementStatus::Failed
                            }
                            Err(QueueError::Packet(_) | QueueError::PacketTooLong { .. }) => {
                                defmt::warn!("Could not build device announcement packet");
                                AnnouncementStatus::Failed
                            }
                        },
                        Err(_) => {
                            defmt::warn!("Could not build device announcement packet");
                            AnnouncementStatus::Failed
                        }
                    }
                }
                None => {
                    defmt::warn!("Could not announce device: device identity unavailable");
                    AnnouncementStatus::NoIdentity
                }
            };
            state.set_announcement_status(status);
        }

        if let Some(pending) = state.take_pending_pairing() {
            pairing.begin(pending.peer_id, pending.key);
            if state
                .device_id
                .is_some_and(|device_id| device_id < pending.peer_id)
            {
                let device_id = state.device_id;
                let packet_id = next_packet_id;
                next_packet_id = next_packet_id.wrapping_add(1);
                let request =
                    build_request_packet(&pending.key, packet_id, device_id.unwrap(), pending.peer_id);
                let queued = request.is_some_and(|(packet, nonce)| {
                    pairing.set_request_nonce(pending.peer_id, nonce);
                    radio_queue.add_packet(&packet).is_ok()
                });
                if queued {
                    state.set_pairing_status(pending.peer_id, PairingUiStatus::RequestSent);
                } else {
                    state.set_pairing_status(pending.peer_id, PairingUiStatus::Failed);
                    pairing.finish(pending.peer_id, false, pending.key);
                }
            }
        }

        if let Some(outgoing) = state.take_pending_message() {
            send_message(&mut state, flash, radio_queue, &mut next_packet_id, outgoing).await;
        }

        if let Some(manage) = state.take_pending_manage() {
            apply_manage(&mut state, flash, manage).await;
        }

        if let Some(offset) = state.take_pending_timezone() {
            let saved = flash
                .with(async |session| {
                    crate::ui::display_settings::save_timezone_offset(
                        &mut session.storage,
                        offset,
                        &mut session.buffer,
                    )
                    .await
                    .is_ok()
                })
                .await;
            if saved {
                defmt::info!("Saved UTC offset {} minutes", offset);
            } else {
                defmt::warn!("Could not save the UTC offset");
            }
        }

        if state.take_pending_delete_all() {
            flash
                .with(async |session| {
                    session
                        .storage
                        .delete_all_conversations(&mut session.buffer)
                        .await
                        .is_ok()
                })
                .await;
            // Every preview line is gone together with the records.
            let kinds = state
                .contacts
                .entries()
                .iter()
                .map(|entry| entry.kind)
                .collect::<alloc::vec::Vec<_>>();
            for kind in kinds {
                state.contacts.seed_preview(kind, "", false, false);
            }
            defmt::info!("Deleted all chat history");
        }

        // Load the four visible rows whenever the conversation changes or the
        // network task reports that history moved.
        let active = state.active_conversation();
        let revision = notify::revision();
        if synced != Some((revision, active)) {
            synced = Some((revision, active));
            if let Some(conversation) = active {
                seed_preview_conversation(&mut state, flash, conversation).await;
                set_chat_rows(&mut state, flash, conversation).await;
            }
        }

        if render::draw_if_changed(
            &mut display,
            &state,
            current_gps,
            &mut previous_state,
            &mut previous_gps,
        )
        .is_err()
        {
            defmt::error!("Display draw failed");
        }
    }
}

/// Build and queue the fragments of one outgoing message, then persist it.
async fn send_message(
    state: &mut UiState,
    flash: &Flash,
    radio_queue: &RadioQueue,
    next_packet_id: &mut u16,
    outgoing: OutgoingMessage,
) {
    let Some(device_id) = state.device_id else {
        defmt::warn!("Could not send message: device identity unavailable");
        return;
    };
    let conversation = outgoing.conversation;
    let peer_id = conversation.peer_id();
    let broadcast = conversation.is_broadcast();
    let contact_key = if broadcast {
        None
    } else {
        state.contacts.direct_key(&peer_id).copied()
    };
    if !broadcast && !outgoing.unencrypted && contact_key.is_none() {
        defmt::warn!("Refusing encrypted message to a peer without a pair key");
        return;
    }
    // Coordinates are always sent as an unencrypted single packet; text is
    // encrypted whenever a pair key exists and the chat asked for encryption.
    let encrypted = !matches!(outgoing.body, OutgoingBody::Coords { .. })
        && contact_key.is_some()
        && !outgoing.unencrypted;

    let packet_id = *next_packet_id;
    let packet_type = match outgoing.body {
        OutgoingBody::Coords { .. } => PacketType::Coordinates,
        OutgoingBody::Text(_) => PacketType::Text,
    };
    let built = build_message_packets(
        packet_id,
        device_id,
        peer_id,
        encrypted,
        packet_type,
        outgoing.payload().as_slice(),
        contact_key,
        state.radio_config.frequency_band.maximum_packet_size(),
    );
    let Some(packets) = built else {
        defmt::warn!("Could not build outgoing message packet");
        return;
    };
    *next_packet_id = next_packet_id.wrapping_add(packets.len() as u16);

    let mut queue_error = None;
    for packet in packets.iter() {
        if let Err(error) = radio_queue.add_packet(packet) {
            queue_error = Some(error);
            break;
        }
    }
    if let Some(error) = queue_error {
        match error {
            QueueError::Full => defmt::warn!("Could not queue message: TX queue is full"),
            QueueError::Packet(_) => defmt::warn!("Could not queue message fragment"),
            QueueError::PacketTooLong { length, maximum } => {
                defmt::warn!("Message fragment {} bytes exceeds {}", length, maximum)
            }
        }
        defmt::warn!("The peer may not receive any fragment of this message");
        return;
    }

    // Persist the sent message so a later ACK can turn it green after reboot.
    let first = packets.first().expect("at least one fragment");
    let stored = StoredMessage::new(
        u32::from(first.message_id),
        Direction::Sent,
        0,
        outgoing.message_kind(),
        outgoing.text(),
        first.packet_id,
        first.message_id,
        device_id,
        encrypted,
    );
    let saved = flash
        .with(async |session| {
            session
                .storage
                .append_messages(&conversation.storage_id(), &[stored], &mut session.buffer)
                .await
                .is_ok()
        })
        .await;
    if saved {
        notify::changed();
        let preview = crate::ui::contact::preview_text(outgoing.text(), outgoing.coordinates());
        state
            .contacts
            .seed_preview(conversation, preview.as_str(), true, false);
    } else {
        defmt::warn!("Could not save the sent message to flash");
    }
}

/// Fragment one outgoing payload and encrypt every fragment when a key exists.
fn build_message_packets(
    packet_id: u16,
    from: [u8; 16],
    to: [u8; 16],
    encrypted: bool,
    packet_type: PacketType,
    plaintext: &[u8],
    key: Option<[u8; crypto::KEY_SIZE]>,
    maximum_packet_size: usize,
) -> Option<HeaplessVec<PacketData, MAX_FRAGMENT_COUNT>> {
    let mut packets = Fragmenter::fragment_with_defaults_and_limit(
        packet_id,
        packet_id,
        from,
        to,
        encrypted,
        packet_type,
        plaintext,
        maximum_packet_size,
    )
    .ok()?;
    if let Some(key) = key
        && encrypted
    {
        for packet in packets.iter_mut() {
            let fragment_plaintext = packet.payload.clone();
            crypto::encrypt_packet_payload(packet, &key, fragment_plaintext.as_slice()).ok()?;
        }
    }
    Some(packets)
}

/// Process one new-message toast: reveal unknown senders in the lists,
/// refresh the row preview, and show the toast unless the peer is blocked.
async fn handle_popup(state: &mut UiState, flash: &Flash, popup: notify::Popup) {
    let conversation = popup.conversation;
    state.note_unknown_sender(conversation);
    // Refresh that row's preview straight from the record just written.
    seed_preview_conversation(state, flash, conversation).await;
    let blocked = state
        .contacts
        .entry_by_kind(conversation)
        .is_some_and(|entry| entry.blocked);
    if blocked {
        return;
    }
    state.show_popup(popup);
}

/// Execute one contact-manage command and return to its source list.
async fn apply_manage(state: &mut UiState, flash: &Flash, manage: ManageCommand) {
    let index = state.contacts.direct_index(&manage.device_id);
    match manage.action {
        ManageAction::Delete => {
            let deleted = flash
                .with(async |session| {
                    session
                        .storage
                        .delete_conversation(&manage.device_id, &mut session.buffer)
                        .await
                        .is_ok()
                })
                .await;
            if let Some(index) = index {
                let _ = state.contacts.remove(index);
            }
            if deleted {
                notify::changed();
            }
            defmt::info!("Deleted one contact and its chat history");
        }
        ManageAction::Block(blocked) => {
            let saved = flash
                .with(async |session| {
                    let Some(mut contact) = session
                        .storage
                        .load_contact(&manage.device_id, &mut session.buffer)
                        .await
                        .ok()
                        .flatten()
                    else {
                        // Unknown senders have no record in flash; the block
                        // lives in the session only.
                        return false;
                    };
                    contact.blocked = blocked;
                    session
                        .storage
                        .save_contact(&contact, &mut session.buffer)
                        .await
                        .is_ok()
                })
                .await;
            if saved || state.contacts.contains_direct(&manage.device_id) {
                state.contacts.set_blocked(&manage.device_id, blocked);
            }
            defmt::info!("Block state saved for one contact: {}", blocked);
        }
    }
    state.after_manage(manage.from_chats);
}

/// Replace the visible chat rows with the newest records of one conversation.
async fn set_chat_rows(state: &mut UiState, flash: &Flash, conversation: ConversationId) {
    let storage_id = conversation.storage_id();
    let records = flash
        .with(async |session| {
            session
                .storage
                .load_recent_messages(&storage_id, MAX_CHAT_MESSAGES, &mut session.buffer)
                .await
                .unwrap_or_default()
        })
        .await;
    let mut rows: HeaplessVec<ChatRow, MAX_CHAT_MESSAGES> = HeaplessVec::new();
    for record in records.iter() {
        let _ = rows.push(ChatRow::from_stored(record));
    }
    state.set_chat_rows(rows);
}

/// Refresh one conversation's chat-list preview from its newest flash record.
async fn seed_preview_conversation(
    state: &mut UiState,
    flash: &Flash,
    conversation: ConversationId,
) {
    let storage_id = conversation.storage_id();
    let newest = flash
        .with(async |session| {
            session
                .storage
                .load_recent_messages(&storage_id, 1, &mut session.buffer)
                .await
                .ok()
                .and_then(|records| records.first().cloned())
        })
        .await;
    let Some(record) = newest else { return };
    let kind = match &record.kind {
        MessageKind::Text => None,
        MessageKind::Coords { latitude_microdegrees, longitude_microdegrees } => {
            Some((*latitude_microdegrees, *longitude_microdegrees))
        }
    };
    let preview = crate::ui::contact::preview_text(record.text.as_str(), kind);
    state
        .contacts
        .seed_preview(conversation, preview.as_str(), record.from_me, record.acknowledged);
}

/// Fill every visible chat-list preview from flash at boot.
async fn seed_previews(state: &mut UiState, flash: &Flash) {
    let mut kinds: heapless::Vec<ConversationId, 33> = heapless::Vec::new();
    for index in state.contacts.visible_indices(false).iter() {
        if let Some(entry) = state.contacts.entry(*index)
            && kinds.push(entry.kind).is_err()
        {
            break;
        }
    }
    for kind in kinds {
        seed_preview_conversation(state, flash, kind).await;
    }
}

fn filter_unsaved_peers(
    contacts: &ContactBook,
    devices: DiscoveryList,
) -> DiscoveryList {
    let mut unsaved = DiscoveryList::new();
    for device_id in devices {
        if contacts.direct_key(&device_id).is_none() {
            let _ = unsaved.push(device_id);
        }
    }
    unsaved
}
