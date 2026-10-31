//! Chat rows, history paging, and the composer text the chat screen edits.

use heapless::{String, Vec};

use crate::storage::message::{ConversationId, MessageType};
use crate::ui::contact::PREVIEW_CHARS;

use super::{MAX_CHAT_MESSAGES, ROW_TEXT_CHARS, Screen, UiState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryPageRequest {
    pub conversation: ConversationId,
    /// `None` loads the latest window; `Some` loads from this record ordinal.
    pub start: Option<u32>,
    /// Absolute record ordinal to select once the page is loaded.
    pub select: Option<u32>,
}

/// One chat row exactly as the four-line panel draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRow {
    pub from_me: bool,
    pub encrypted: bool,
    pub acknowledged: bool,
    pub coordinates: Option<(i32, i32)>,
    /// Already truncated to `ROW_TEXT_CHARS`; empty for a coordinate row.
    pub text: String<ROW_TEXT_CHARS>,
    /// Broadcast rows name the sending peer; direct rows keep the FROM label.
    pub sender_label: Option<String<32>>,
}

impl ChatRow {
    /// Trim one flash record down to the fields the panel draws.
    /// `sender_label` comes from the contact book for broadcast conversations.
    pub fn from_stored(
        message: &crate::storage::message::StoredMessage,
        sender_label: Option<String<32>>,
    ) -> Self {
        let mut row = Self {
            from_me: message.from_me,
            encrypted: message.encrypted,
            acknowledged: message.acknowledged,
            coordinates: message.kind.coordinates(),
            text: String::new(),
            sender_label,
        };
        let source = match &message.kind {
            MessageType::Text => message.text.as_str(),
            MessageType::Coords { .. } => "[GPS]",
        };
        let mut preview = String::<PREVIEW_CHARS>::new();
        let _ = preview.push_str(source);
        for character in preview.chars().take(ROW_TEXT_CHARS) {
            if row.text.push(character).is_err() {
                break;
            }
        }
        row
    }
}

impl UiState {
    /// Whether the open chat targets a contact with a pair key.
    pub(super) fn active_chat_is_encrypted(&self) -> bool {
        match &self.screen {
            Screen::ChatView {
                conversation: ConversationId::Direct(device_id),
                ..
            } => self.contacts.direct_key(device_id).is_some(),
            _ => false,
        }
    }

    /// Conversation of one CHATS row; `None` past the end of the list.
    pub(super) fn chat_row_conversation(&self, row: usize) -> Option<ConversationId> {
        self.contacts
            .chat_indices()
            .get(row)
            .and_then(|index| self.contacts.entry(*index))
            .map(|entry| entry.conversation)
    }

    /// Replace the visible history window and its absolute flash position.
    pub fn set_chat_page(
        &mut self,
        rows: Vec<ChatRow, MAX_CHAT_MESSAGES>,
        base: u32,
        start: u32,
        total: u32,
        selected_message: Option<usize>,
    ) {
        self.messages = rows;
        if let Screen::ChatView {
            history_base,
            history_start,
            history_total,
            selected_message: selected,
            ..
        } = &mut self.screen
        {
            *history_base = base;
            *history_start = start;
            *history_total = total;
            *selected = selected_message.filter(|index| *index < self.messages.len());
        }
    }

    /// Clear the composer after a successful send.
    pub fn clear_chat_input(&mut self) {
        self.chat_input.clear();
        if let Screen::ChatView {
            cursor,
            pending_hex,
            ..
        } = &mut self.screen
        {
            *cursor = 0;
            *pending_hex = None;
        }
    }
}
