//! Application UI state and display rendering.

pub mod contact;
pub mod display_settings;
#[cfg(feature = "fake-navigator")]
pub mod fake_navigator;
pub mod navigation;
pub mod render;
pub mod state;

use crate::radio::types::RadioConfig;
use crate::{
    crypto,
    network::{
        discovery::DiscoveryList,
        message_delivery::IncomingMessageQueue,
        pairing::{PairingCoordinator, PairingEvent, build_request_packet},
    },
    packet::{
        fragment::Fragmenter,
        packet_data::{DEFAULT_HOP_LIMIT, MAX_FRAGMENT_COUNT, PacketData},
        packet_type::PacketType,
    },
    radio::queue::{QueueError, RadioQueue},
};
use embassy_futures::select::{Either, select};
use state::{AnnouncementStatus, PairingUiStatus, UiState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpsSnapshot {
    pub fix: bool,
    pub latitude_microdegrees: i32,
    pub longitude_microdegrees: i32,
    pub hour: u8,
    pub minute: u8,
    pub has_time: bool,
}

/// 1/ Convert one GPS coordinate to signed microdegrees for compact display text.
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
    }
}

/// 2/ Render the current UI model with a consistent, always-visible GPS bar.
pub fn draw<D>(display: &mut D, state: &UiState, gps: GpsSnapshot) -> Result<(), D::Error>
where
    D: embedded_graphics::prelude::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
{
    render::draw(display, state, gps)
}

/// 3/ Render the current page whenever the shared GPS status changes.
pub async fn run<'a, D>(
    mut display: D,
    device_id: Option<[u8; 16]>,
    contacts: crate::ui::contact::ContactBook,
    firmware_version: &'static str,
    pairing: &PairingCoordinator,
    incoming_messages: &'a IncomingMessageQueue,
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
) -> !
where
    D: embedded_graphics::prelude::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
{
    let initial_radio_config = radio_config.get().await;
    let initial_discoveries = filter_unsaved_peers(&contacts, discovery.get().await);
    let mut state = UiState::new(
        device_id,
        contacts,
        firmware_version,
        initial_radio_config,
        initial_discoveries,
    );

    #[cfg(feature = "fake-navigator")]
    let mut fake_navigator = fake_navigator::FakeNavigator::new();
    let mut current_gps = gps.get().await;
    state.update_gps_snapshot(current_gps);

    let mut previous_state = None;
    let mut previous_gps = None;
    let mut next_packet_id = 0x8000u16;
    loop {
        #[cfg(feature = "fake-navigator")]
        match select(
            select(
                select(gps.changed(), radio_config.changed()),
                select(
                    select(discovery.changed(), pairing_events.changed()),
                    incoming_messages.receive(),
                ),
            ),
            fake_navigator.next_key(),
        )
        .await
        {
            Either::First(Either::First(Either::First(snapshot))) => {
                current_gps = snapshot;
                state.update_gps_snapshot(snapshot);
            }
            Either::First(Either::First(Either::Second(config))) => state.radio_config = config,
            Either::First(Either::Second(Either::First(Either::First(devices)))) => {
                state.discovered_devices = filter_unsaved_peers(&state.contacts, devices)
            }
            Either::First(Either::Second(Either::First(Either::Second(event)))) => {
                if let Some(event) = event {
                    state.handle_pairing_event(event);
                }
            }
            Either::First(Either::Second(Either::Second(message))) => {
                state.record_incoming_message(message);
            }
            Either::Second(key) => {
                state.handle_key(key);
            }
        }
        #[cfg(not(feature = "fake-navigator"))]
        match select(
            select(gps.changed(), radio_config.changed()),
            select(
                select(discovery.changed(), pairing_events.changed()),
                incoming_messages.receive(),
            ),
        )
        .await
        {
            Either::First(Either::First(snapshot)) => {
                current_gps = snapshot;
                state.update_gps_snapshot(snapshot);
            }
            Either::First(Either::Second(config)) => state.radio_config = config,
            Either::Second(Either::First(Either::First(devices))) => {
                state.discovered_devices = filter_unsaved_peers(&state.contacts, devices)
            }
            Either::Second(Either::First(Either::Second(event))) => {
                if let Some(event) = event {
                    state.handle_pairing_event(event);
                }
            }
            Either::Second(Either::Second(message)) => {
                state.record_incoming_message(message);
            }
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
                        crate::packet::packet_type::PacketType::DeviceAnnounce,
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
                let device_id = state.device_id.unwrap();
                let packet_id = next_packet_id;
                next_packet_id = next_packet_id.wrapping_add(1);
                let request =
                    build_request_packet(&pending.key, packet_id, device_id, pending.peer_id);
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
            match state.device_id {
                Some(device_id) => {
                    let contact_key = state.contacts.direct_key(&outgoing.to).copied();
                    let allowed = if outgoing.unencrypted {
                        contact_key.is_none()
                    } else {
                        contact_key.is_some()
                    };
                    if !allowed {
                        defmt::warn!(
                            "Refusing direct message with missing or mismatched encryption state"
                        );
                        continue;
                    }
                    let packet_id = next_packet_id;
                    match build_direct_message_packets(
                        packet_id,
                        device_id,
                        outgoing.to,
                        outgoing.text.as_bytes(),
                        contact_key,
                        state.radio_config.frequency_band.maximum_packet_size(),
                    ) {
                        Some(packets) => {
                            next_packet_id = next_packet_id.wrapping_add(packets.len() as u16);
                            let mut queue_error = None;
                            for packet in packets.iter() {
                                if let Err(error) = radio_queue.add_packet(packet) {
                                    queue_error = Some(error);
                                    break;
                                }
                            }
                            if queue_error.is_none() {
                                state.clear_chat_input();
                                state.record_outgoing_message(
                                    outgoing.to,
                                    device_id,
                                    packet_id,
                                    outgoing.text.as_str(),
                                    contact_key.is_some(),
                                );
                                defmt::info!("Queued {}-fragment direct message", packets.len());
                            } else {
                                match queue_error.unwrap() {
                                    QueueError::Full => defmt::warn!(
                                        "Could not queue all message fragments: TX queue is full"
                                    ),
                                    QueueError::Packet(_) => defmt::warn!(
                                        "Could not queue message fragment: packet is invalid"
                                    ),
                                    QueueError::PacketTooLong { length, maximum } => {
                                        defmt::warn!(
                                            "Message fragment {} bytes exceeds {}",
                                            length,
                                            maximum
                                        )
                                    }
                                }
                                defmt::warn!("Partial message may not be reassembled by the peer");
                            }
                        }
                        None => defmt::warn!("Could not build or encrypt direct message packet"),
                    }
                }
                None => defmt::warn!("Could not send direct message: device identity unavailable"),
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

fn build_direct_message_packets(
    packet_id: u16,
    from: [u8; 16],
    to: [u8; 16],
    plaintext: &[u8],
    key: Option<[u8; crypto::KEY_SIZE]>,
    maximum_packet_size: usize,
) -> Option<heapless::Vec<PacketData, MAX_FRAGMENT_COUNT>> {
    let encrypted = key.is_some();
    let mut packets = Fragmenter::fragment_with_defaults_and_limit(
        packet_id,
        packet_id,
        from,
        to,
        encrypted,
        PacketType::Text,
        plaintext,
        maximum_packet_size,
    )
    .ok()?;
    if let Some(key) = key {
        for packet in packets.iter_mut() {
            let fragment_plaintext = packet.payload.clone();
            crypto::encrypt_packet_payload(packet, &key, fragment_plaintext.as_slice()).ok()?;
        }
    }
    Some(packets)
}

fn filter_unsaved_peers(
    contacts: &crate::ui::contact::ContactBook,
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
