//! Pending commands the run loop drains, and the status reports it writes back.

use crate::network::types::PairingEvent;
use crate::storage::contact::Contact;
use crate::storage::message::ConversationId;

use super::{
    FrequencyUiStatus, HistoryPageRequest, ManageCommand, OutgoingMessage, PairingCommand,
    PairingUiStatus, RenameCommand, ResetUiStatus, Screen, UiState,
};

impl UiState {
    /// Nudge the UTC offset by one step and queue the new value for flash.
    pub(super) fn adjust_timezone_offset(&mut self, delta_minutes: i32) {
        if let Some(next) = self.settings.adjust_timezone_offset(delta_minutes) {
            self.pending_timezone = Some(next);
        }
    }

    /// The UTC offset whose new value the run loop still has to persist.
    pub fn take_pending_timezone(&mut self) -> Option<i32> {
        self.pending_timezone.take()
    }

    /// Remember a peer the book has never seen so it appears in the list.
    /// Direct conversations only; broadcast is always present.
    pub fn note_unknown_sender(&mut self, conversation: ConversationId) {
        if let ConversationId::Direct(device_id) = conversation {
            self.contacts.note_unknown(device_id);
        }
    }

    /// Keep the latest fix and estimate course after at least five metres of movement.
    pub fn update_gps_snapshot(&mut self, snapshot: crate::ui::GpsSnapshot) {
        self.gps_snapshot = snapshot;
        if !snapshot.fix {
            self.course_anchor = None;
            self.movement_course_degrees = None;
            return;
        }

        let current = (
            snapshot.latitude_microdegrees,
            snapshot.longitude_microdegrees,
        );
        let Some(anchor) = self.course_anchor else {
            self.course_anchor = Some(current);
            return;
        };
        // Estimate course only after the user has moved far enough to reduce GPS drift.
        if let Some((distance, bearing)) = super::navigation::distance_and_bearing(anchor, current)
            && distance >= 5.0
        {
            self.movement_course_degrees = Some(bearing);
            self.course_anchor = Some(current);
        }
    }

    pub fn take_pending_message(&mut self) -> Option<OutgoingMessage> {
        self.pending_message.take()
    }

    pub fn take_pending_manage(&mut self) -> Option<ManageCommand> {
        self.pending_manage.take()
    }

    pub fn take_pending_delete_all(&mut self) -> bool {
        core::mem::replace(&mut self.pending_delete_all, false)
    }

    /// The conversation whose chat panel is open, if any.
    pub fn active_conversation(&self) -> Option<ConversationId> {
        match &self.screen {
            Screen::ChatView { conversation, .. } => Some(*conversation),
            _ => None,
        }
    }

    /// Take a request to load an adjacent page of the open conversation.
    pub fn take_pending_history_page(&mut self) -> Option<HistoryPageRequest> {
        self.pending_history_page.take()
    }

    /// Show one toast over whatever screen is active. A toast that arrives
    /// while another is visible waits instead of replacing it, and a retune
    /// request is never displaced by ordinary message traffic.
    pub fn show_popup(&mut self, popup: crate::notify::Popup) {
        if self.popup.is_none() {
            self.popup = Some(popup);
            return;
        }
        let waiting_is_request = self.deferred_popup.as_ref().is_some_and(|waiting| {
            matches!(
                waiting.popup_type,
                crate::notify::PopupType::FrequencyChangeRequest { .. }
            )
        });
        // A waiting request is kept: a newer toast cannot push it out, and a
        // second request waits behind the first. Otherwise the newest wins.
        if !waiting_is_request {
            self.deferred_popup = Some(popup);
        }
    }

    /// Close the visible toast and promote the one that was waiting.
    pub(crate) fn dismiss_popup(&mut self) {
        self.popup = self.deferred_popup.take();
    }

    /// Return to the list whose contact rows just changed, dropping stale picks.
    pub fn after_manage(&mut self, from_chats: bool) {
        self.screen = if from_chats {
            Screen::ChatsList { selected: 0 }
        } else {
            Screen::ContactsList { selected: 0 }
        };
    }

    pub fn take_pending_announcement(&mut self) -> bool {
        core::mem::replace(&mut self.pending_announcement, false)
    }

    /// The contact name the run loop still has to persist.
    pub fn take_pending_rename(&mut self) -> Option<RenameCommand> {
        self.pending_rename.take()
    }

    /// True once the user left the scan list and repeats must stop.
    pub fn take_pending_scan_cancel(&mut self) -> bool {
        core::mem::replace(&mut self.cancel_scan, false)
    }

    /// One peer the run loop should send an add-contact request to.
    pub fn take_pending_contact_request(&mut self) -> Option<[u8; 16]> {
        self.pending_contact_request.take()
    }

    pub fn take_pending_pairing(&mut self) -> Option<PairingCommand> {
        self.pending_pairing.take()
    }

    /// The identity reset the run loop still has to execute.
    pub fn take_pending_reset_identity(&mut self) -> bool {
        core::mem::replace(&mut self.pending_reset_identity, false)
    }

    /// The preset the run loop still has to send to the radio worker.
    pub fn take_pending_frequency(&mut self) -> Option<u32> {
        self.pending_frequency.take()
    }

    /// The frequency and contact mask of the requests still to send to peers.
    pub fn take_pending_frequency_requests(&mut self) -> Option<(u32, u32)> {
        self.pending_frequency_requests.take()
    }

    /// Report the outcome of the requested identity regeneration.
    pub fn set_identity_reset_status(&mut self, status: ResetUiStatus) {
        if let Screen::IdentityReset {
            status: current_status,
        } = &mut self.screen
        {
            *current_status = status;
        }
    }

    /// Report the outcome of the requested retune.
    pub fn set_frequency_status(&mut self, status: FrequencyUiStatus) {
        if let Screen::FrequencyStatus {
            status: current_status,
        } = &mut self.screen
        {
            *current_status = status;
        }
    }

    pub fn set_pairing_status(&mut self, device_id: [u8; 16], status: PairingUiStatus) {
        if let Screen::PairingStatus {
            device_id: current,
            status: current_status,
        } = &mut self.screen
            && *current == device_id
        {
            *current_status = status;
        }
    }

    pub fn handle_pairing_event(&mut self, event: PairingEvent) {
        if event.succeeded {
            let name = self.pending_contact_name.clone();
            let contact = Contact {
                device_id: event.peer_id,
                key: event.key,
                custom_name: name.clone(),
                blocked: false,
            };
            self.paired_contact(&contact);
            // The handshake saves the contact without a name, so write it now.
            if !name.is_empty() {
                self.pending_rename = Some(RenameCommand {
                    device_id: event.peer_id,
                    name,
                });
            }
        } else {
            self.set_pairing_status(event.peer_id, PairingUiStatus::Failed);
        }
        self.pending_contact_name.clear();
    }

    /// Insert or update the contact for a successful pairing.
    pub fn paired_contact(&mut self, contact: &Contact) {
        let _ = self.contacts.adopt(contact);
        self.discovered_devices
            .retain(|device_id| *device_id != contact.device_id);
        self.set_pairing_status(contact.device_id, PairingUiStatus::Paired);
    }
}
