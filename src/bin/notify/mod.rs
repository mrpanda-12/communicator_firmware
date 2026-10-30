//! Receive-path notifications for the UI task.
//!
//! The receive path never touches the display and never owns chat history:
//! it writes accepted messages to flash and tells the UI what changed. The UI
//! polls flash only when a notification arrives, so the screen stays cheap and
//! there is no second copy of the messages in RAM.

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
    watch::{Receiver, Watch},
};
use heapless::String;

use crate::storage::message::ConversationId;

/// Characters of one popup preview line.
pub const POPUP_TEXT_CHARS: usize = 34;
/// Characters of the sender name above that line.
pub const POPUP_NAME_CHARS: usize = 32;

/// What a toast reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupType {
    NewMessage,
    Alert,
    /// A peer asked to be added; nothing is stored and no key is made.
    ContactRequest,
}

/// One toast: which conversation, the sender name, and a short line of text.
#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    pub conversation: ConversationId,
    pub popup_type: PopupType,
    /// Who sent the message; zeroed for local alerts.
    pub sender: [u8; 16],
    /// Sender name, or the device ID when the peer has no name yet.
    pub name: String<POPUP_NAME_CHARS>,
    pub text: String<POPUP_TEXT_CHARS>,
}

/// Popups that may wait for the UI; a full queue drops the oldest toast.
pub static POPUPS: Channel<CriticalSectionRawMutex, Popup, 8> = Channel::new();
/// Fires whenever chat history may have changed (accept, ACK, delete).
pub static CHANGED: Watch<CriticalSectionRawMutex, u32, 8> = Watch::new();

static REVISION: AtomicU32 = AtomicU32::new(0);

/// Cut one line to the popup width, marking a cut with an ellipsis.
fn fit<const N: usize>(text: &str) -> String<N> {
    let mut line = String::new();
    if text.chars().count() <= N {
        let _ = line.push_str(text);
        return line;
    }
    for character in text.chars().take(N - 3) {
        if line.push(character).is_err() {
            break;
        }
    }
    let _ = line.push_str("...");
    line
}

/// Record one accepted message: queue a popup toast and bump the revision.
pub fn receive(conversation: ConversationId, from: [u8; 16], text: &str) {
    let popup = Popup {
        conversation,
        popup_type: PopupType::NewMessage,
        sender: from,
        name: String::new(),
        text: fit(text),
    };
    // A full queue drops the oldest toast so the newest message is still shown.
    if POPUPS.free_capacity() == 0 {
        let receiver = POPUPS.receiver();
        let _ = receiver.try_receive();
    }
    let _ = POPUPS.try_send(popup);
    changed();
}

/// Warn that a flash append could not be stored.
pub fn storage_full(conversation: ConversationId) {
    alert(conversation, "STORAGE FULL");
}

/// Tell the user a peer asked to be added; this pairs nothing by itself.
pub fn contact_request(from: [u8; 16]) {
    let popup = Popup {
        conversation: ConversationId::from_peer(from),
        popup_type: PopupType::ContactRequest,
        sender: from,
        name: String::new(),
        text: fit("WANTS TO ADD YOU"),
    };
    if POPUPS.free_capacity() == 0 {
        let receiver = POPUPS.receiver();
        let _ = receiver.try_receive();
    }
    let _ = POPUPS.try_send(popup);
}

/// Warn that the outbox is full and the message stayed in the composer.
pub fn send_queue_full(conversation: ConversationId) {
    alert(conversation, "SEND QUEUE FULL");
}

fn alert(conversation: ConversationId, text: &str) {
    let popup = Popup {
        conversation,
        popup_type: PopupType::Alert,
        sender: [0; 16],
        name: String::new(),
        text: fit(text),
    };
    if POPUPS.free_capacity() == 0 {
        let receiver = POPUPS.receiver();
        let _ = receiver.try_receive();
    }
    let _ = POPUPS.try_send(popup);
}

/// Signal the UI that history changed without an accompanying toast.
pub fn changed() {
    let revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
    CHANGED.sender().send(revision);
}

/// Current history revision; the UI compares this to its loaded snapshot.
pub fn revision() -> u32 {
    REVISION.load(Ordering::Relaxed)
}

pub fn revision_receiver() -> Receiver<'static, CriticalSectionRawMutex, u32, 8> {
    CHANGED
        .receiver()
        .unwrap_or_else(|| crate::fatal("history revision watch has no free receiver"))
}

pub fn popup_receiver() -> embassy_sync::channel::DynamicReceiver<'static, Popup> {
    POPUPS.receiver().into()
}
