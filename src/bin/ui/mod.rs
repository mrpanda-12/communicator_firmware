//! Application UI state and display rendering.
//!
//! Screen text and chat rows come from flash; the network task only publishes
//! notifications here, so there is never a second copy of chat history in RAM.

pub mod contact;
pub mod display_settings;
pub mod navigation;
pub mod render;
pub mod state;

use crate::DEVICE_ID;
use crate::crypto;
use crate::hardware::keypad::KeypadQueue;
use crate::network::{
    outbox,
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
    control::RadioControl,
    queue::{QueueError, RadioQueue},
    radio_error::RadioError,
    types::RadioConfig,
};
use crate::storage::message::{ConversationId, Direction, StoredMessage};
use crate::storage::session::Flash;
use crate::ui::{
    contact::ContactBook,
    state::{HistoryPageRequest, ManageAction, ManageCommand, OutgoingBody, OutgoingMessage},
};
use embassy_futures::select::{Either, select};
use esp_hal::rng::Trng;
use heapless::Vec as HeaplessVec;
use state::MAX_CHAT_MESSAGES;
use state::{
    AnnouncementStatus, ChatRow, FrequencyUiStatus, PairingUiStatus, ResetUiStatus, UiState,
};

/// A broadcast row names the sender; unknown peers render a short ID instead.
fn broadcast_sender_label(contacts: &ContactBook, sender: &[u8; 16]) -> heapless::String<32> {
    match contacts.entry_by_kind(ConversationId::Direct(*sender)) {
        Some(entry) => entry.display_name(),
        None => {
            let mut label = heapless::String::<32>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut label,
                format_args!("UNHEARD {:02X}{:02X}", sender[0], sender[1]),
            );
            label
        }
    }
}

/// List rows that can have a conversation: every contact slot, including the
/// broadcast entry that always occupies the first one.
const MAX_VISIBLE_CONVERSATIONS: usize = contact::MAX_CONTACTS;

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

/// 3/ React to screen events, chat storage, and received-message notifications.
#[allow(clippy::too_many_arguments)]
pub async fn run<'a, D>(
    mut display: D,
    device_id: Option<[u8; 16]>,
    contacts: ContactBook,
    firmware_version: &'static str,
    timezone_offset_minutes: i32,
    pairing: &PairingCoordinator,
    flash: &'a Flash,
    radio_control: &RadioControl,
    trng: &Trng,
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

    let mut current_gps = gps.get().await;
    state.update_gps_snapshot(current_gps);

    // Fill every visible chat row preview straight from flash at boot.
    seed_previews(&mut state, flash).await;

    let popups = notify::popup_receiver();
    let mut changes = notify::revision_receiver();
    let mut previous_state = None;
    let mut previous_gps = None;
    let mut synced = None;

    loop {
        match select(
            select(
                select(gps.changed(), radio_config.changed()),
                select(discovery.changed(), pairing_events.changed()),
            ),
            select(
                select(popups.receive(), changes.changed()),
                keypad_events.receive(),
            ),
        )
        .await
        {
            Either::First(Either::First(Either::First(snapshot))) => {
                current_gps = snapshot;
                state.update_gps_snapshot(snapshot);
            }
            Either::First(Either::First(Either::Second(config))) => {
                state.radio_config = config;
            }
            Either::First(Either::Second(Either::First(devices))) => {
                state.discovered_devices = filter_unsaved_peers(&state.contacts, devices);
            }
            Either::First(Either::Second(Either::Second(event))) => {
                if let Some(event) = event {
                    state.handle_pairing_event(event);
                }
            }
            Either::Second(Either::First(Either::First(popup))) => {
                handle_popup(&mut state, flash, popup).await;
            }
            // A storage-revision bump needs no immediate action; the loop
            // reads notify::revision() below on the next pass.
            Either::Second(Either::First(Either::Second(_))) => {}
            Either::Second(Either::Second(key)) => state.handle_keypad(key),
        }

        if state.take_pending_announcement() {
            let status = match state.device_id {
                Some(device_id) => {
                    let packet_id = flash.reserve_packet_ids(1).await;
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
                let packet_id = flash.reserve_packet_ids(1).await;
                let queued = match build_request_packet(
                    &pending.key,
                    packet_id,
                    device_id.unwrap(),
                    pending.peer_id,
                ) {
                    Some((packet, nonce)) => {
                        pairing.set_request_nonce(pending.peer_id, nonce);
                        radio_queue.add_packet(&packet).is_ok()
                    }
                    None => false,
                };
                if queued {
                    state.set_pairing_status(pending.peer_id, PairingUiStatus::RequestSent);
                } else {
                    state.set_pairing_status(pending.peer_id, PairingUiStatus::Failed);
                    pairing.finish(pending.peer_id, false, pending.key);
                }
            }
        }

        if let Some(outgoing) = state.take_pending_message() {
            send_message(&mut state, flash, outgoing).await;
        }

        if let Some(manage) = state.take_pending_manage() {
            apply_manage(&mut state, flash, manage).await;
        }

        if state.take_pending_reset_identity() {
            let new_id = flash
                .with(async |session| {
                    session
                        .storage
                        .regenerate_device_id(&mut session.buffer, |id| {
                            trng.read(id);
                            Ok(())
                        })
                        .await
                })
                .await;
            match new_id {
                Ok(new_id) => {
                    defmt::info!("Regenerated and saved this device identity");
                    state.device_id = Some(new_id);
                    // The radio worker reads the same watch, so the next frame
                    // is addressed from the new identity.
                    DEVICE_ID.sender().send(Some(new_id));
                    state.set_identity_reset_status(ResetUiStatus::Done);
                    // Contacts and chat history belong to the person, not the
                    // device identity, so they are kept as they are.
                }
                Err(_) => {
                    defmt::warn!("Could not regenerate the device identity");
                    state.set_identity_reset_status(ResetUiStatus::Failed);
                }
            }
        }

        if let Some(frequency_hz) = state.take_pending_frequency() {
            let status = match radio_control.request_frequency_change(frequency_hz) {
                Ok(()) => {
                    defmt::info!("Requested frequency change to {} Hz", frequency_hz);
                    FrequencyUiStatus::Queued
                }
                Err(RadioError::RadioBusy | RadioError::InvalidConfiguration) => {
                    defmt::warn!("Frequency change not accepted for {} Hz", frequency_hz);
                    FrequencyUiStatus::Busy
                }
                Err(_) => FrequencyUiStatus::Busy,
            };
            state.set_frequency_status(status);
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

        // Service an explicit arrow-key history page request first.
        if let Some(request) = state.take_pending_history_page() {
            load_conversation_page(&mut state, flash, request).await;
            synced = Some((notify::revision(), state.active_conversation()));
        }

        // Reload the latest page when the conversation changes or history moves.
        let active = state.active_conversation();
        let revision = notify::revision();
        if synced != Some((revision, active)) {
            synced = Some((revision, active));
            if let Some(conversation) = active {
                open_conversation(&mut state, flash, conversation).await;
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
async fn send_message(state: &mut UiState, flash: &Flash, outgoing: OutgoingMessage) {
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
    // Text and coordinates are encrypted whenever this chat has a pair key and
    // the chat asked for encryption; broadcast and unpaired traffic stays
    // plaintext, and coordinates stay a single fragment.
    let encrypted = contact_key.is_some() && !outgoing.unencrypted;

    let packet_type = match outgoing.body {
        OutgoingBody::Coords { .. } => PacketType::Coordinates,
        OutgoingBody::Text(_) => PacketType::Text,
    };
    let payload = outgoing.payload();
    let fragment_count = match Fragmenter::fragment_count_with_limit(
        encrypted,
        payload.len(),
        state.radio_config.frequency_band.maximum_packet_size(),
    ) {
        Ok(count) => count,
        Err(_) => {
            defmt::warn!("Could not size outgoing message packets");
            return;
        }
    };
    let packet_id = flash.reserve_packet_ids(fragment_count).await;
    let built = build_message_packets(
        packet_id,
        device_id,
        peer_id,
        encrypted,
        packet_type,
        payload.as_slice(),
        contact_key,
        state.radio_config.frequency_band.maximum_packet_size(),
    );
    let Some(packets) = built else {
        defmt::warn!("Could not build outgoing message packet");
        return;
    };
    let Some(first) = packets.first() else { return };
    let message_id = first.message_id;
    let packet_id = first.packet_id;

    // Hand the whole message to the outbox, so a full TX queue delays it
    // instead of dropping it.
    if outbox::submit(message_id, packets, encrypted).is_err() {
        defmt::warn!("Could not queue message: two sends are already waiting");
        notify::send_queue_full(conversation);
        return;
    }

    // Persist the sent message so a later ACK can turn it green after reboot.
    let stored = StoredMessage::new(
        message_id,
        Direction::Sent,
        0,
        outgoing.message_type(),
        outgoing.text(),
        packet_id,
        message_id,
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
        // The draft is sent and stored; do not leave it in the composer to resend.
        state.clear_chat_input();
    } else {
        defmt::warn!("Could not save the sent message to flash");
        notify::storage_full(conversation);
    }
}

/// Fragment one outgoing payload and encrypt every fragment when a key exists.
fn build_message_packets(
    packet_id: u32,
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
    if popup.kind == notify::PopupKind::Alert {
        state.show_popup(popup);
        return;
    }
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

/// Load the newest page when a conversation is opened.
async fn open_conversation(state: &mut UiState, flash: &Flash, conversation: ConversationId) {
    load_conversation_page(
        state,
        flash,
        HistoryPageRequest {
            conversation,
            start: None,
            select: None,
        },
    )
    .await;
}

/// Load one indexed page and preserve its position in the complete history.
async fn load_conversation_page(state: &mut UiState, flash: &Flash, request: HistoryPageRequest) {
    let storage_id = request.conversation.storage_id();
    let page = flash
        .with(async |session| {
            session
                .storage
                .load_message_page(
                    &storage_id,
                    request.start,
                    MAX_CHAT_MESSAGES,
                    &mut session.buffer,
                )
                .await
                .unwrap_or((0, 0, 0, alloc::vec::Vec::new()))
        })
        .await;
    let (total, base, start, records) = page;
    let mut rows: HeaplessVec<ChatRow, MAX_CHAT_MESSAGES> = HeaplessVec::new();
    for record in records.iter() {
        let sender_label = match &request.conversation {
            ConversationId::Broadcast if !record.from_me => {
                Some(broadcast_sender_label(&state.contacts, &record.sender_id))
            }
            _ => None,
        };
        let _ = rows.push(ChatRow::from_stored(record, sender_label));
    }
    if request.start.is_none()
        && let Some(newest) = records.last()
    {
        let preview =
            crate::ui::contact::preview_text(newest.text.as_str(), newest.kind.coordinates());
        state.contacts.seed_preview(
            request.conversation,
            preview.as_str(),
            newest.from_me,
            newest.acknowledged,
        );
    }
    let selected = request
        .select
        .and_then(|ordinal| ordinal.checked_sub(start))
        .and_then(|index| usize::try_from(index).ok());
    state.set_chat_page(rows, base, start, total, selected);
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
    let preview = crate::ui::contact::preview_text(record.text.as_str(), record.kind.coordinates());
    state.contacts.seed_preview(
        conversation,
        preview.as_str(),
        record.from_me,
        record.acknowledged,
    );
}

/// Fill every visible chat-list preview from flash at boot.
async fn seed_previews(state: &mut UiState, flash: &Flash) {
    let mut kinds: heapless::Vec<ConversationId, MAX_VISIBLE_CONVERSATIONS> = heapless::Vec::new();
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

fn filter_unsaved_peers(contacts: &ContactBook, devices: DiscoveryList) -> DiscoveryList {
    let mut unsaved = DiscoveryList::new();
    for device_id in devices {
        if contacts.direct_key(&device_id).is_none() {
            let _ = unsaved.push(device_id);
        }
    }
    unsaved
}
