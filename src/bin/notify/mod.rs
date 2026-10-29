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
pub const POPUP_TEXT_CHARS: usize = 24;

/// What a toast reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupKind {
    NewMessage,
    Alert,
}

/// One toast: which conversation and a short line of text.
#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    pub conversation: ConversationId,
    pub kind: PopupKind,
    pub text: String<POPUP_TEXT_CHARS>,
}

/// Popups that may wait for the UI; a full queue drops the oldest toast.
pub static POPUPS: Channel<CriticalSectionRawMutex, Popup, 8> = Channel::new();
/// Fires whenever chat history may have changed (accept, ACK, delete).
pub static CHANGED: Watch<CriticalSectionRawMutex, u32, 8> = Watch::new();

static REVISION: AtomicU32 = AtomicU32::new(0);

/// Record one accepted message: queue a popup toast and bump the revision.
pub fn receive(conversation: ConversationId, preview: &str) {
    let mut popup = Popup {
        conversation,
        kind: PopupKind::NewMessage,
        text: String::new(),
    };
    let _ = popup.text.push_str(preview);
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

/// Warn that the outbox is full and the message stayed in the composer.
pub fn send_queue_full(conversation: ConversationId) {
    alert(conversation, "SEND QUEUE FULL");
}

fn alert(conversation: ConversationId, text: &str) {
    let mut popup = Popup {
        conversation,
        kind: PopupKind::Alert,
        text: String::new(),
    };
    let _ = popup.text.push_str(text);
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
    CHANGED.receiver().unwrap()
}

pub fn popup_receiver() -> embassy_sync::channel::DynamicReceiver<'static, Popup> {
    POPUPS.receiver().into()
}
