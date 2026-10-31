//! Keypad keys: hex-pair character entry and the BACK editing rule.

use heapless::String;

use crate::hardware::keypad::KeypadKey;

use super::{Key, Screen, UiState};

impl UiState {
    /// Apply one 5x5 keypad key. Two hex digits type one ASCII character, so
    /// a lone digit stays pending until its pair arrives or BACK clears it.
    pub fn handle_keypad(&mut self, key: KeypadKey) {
        if key == KeypadKey::Help {
            self.help_open = !self.help_open;
            return;
        }
        // ENTER accepts or acknowledges a toast; BACK dismisses it.
        if self.popup.is_some() {
            match key {
                KeypadKey::Enter | KeypadKey::Send => self.handle_key(Key::Enter),
                KeypadKey::Back => self.handle_key(Key::Back),
                _ => {}
            }
            return;
        }
        if self.help_open {
            if matches!(key, KeypadKey::Back | KeypadKey::Enter) {
                self.help_open = false;
            }
            return;
        }
        match key {
            KeypadKey::Digit(digit) => match &mut self.screen {
                Screen::ChatView {
                    pending_hex,
                    cursor,
                    ..
                } => Self::type_hex_digit(pending_hex, &mut self.chat_input, cursor, digit),
                Screen::NameEntry {
                    pending_hex,
                    name,
                    cursor,
                    ..
                } => Self::type_hex_digit(pending_hex, name, cursor, digit),
                Screen::SecretEntry {
                    pending_hex,
                    secret,
                    cursor,
                    ..
                } => Self::type_hex_digit(pending_hex, secret, cursor, digit),
                _ => {}
            },
            KeypadKey::Back => self.handle_keypad_back(),
            KeypadKey::Send | KeypadKey::Enter => self.handle_key(Key::Enter),
            KeypadKey::Up => self.handle_key(Key::Up),
            KeypadKey::Down => self.handle_key(Key::Down),
            KeypadKey::Left => self.handle_key(Key::Left),
            KeypadKey::Right => self.handle_key(Key::Right),
            // Handled above, before the overlay gate.
            KeypadKey::Help | KeypadKey::Unassigned => {}
        }
    }

    /// Commit a hex pair to the focused field and leave a lone digit pending.
    fn type_hex_digit<const N: usize>(
        pending_hex: &mut Option<u8>,
        text: &mut String<N>,
        cursor: &mut usize,
        digit: u8,
    ) {
        let digit = digit & 0x0F;
        match pending_hex.take() {
            None => *pending_hex = Some(digit),
            Some(high) => {
                let character = char::from((high << 4) | digit);
                if character.is_ascii() && !character.is_control() && text.push(character).is_ok() {
                    *cursor = text.len();
                }
            }
        }
    }

    /// BACK clears a pending nibble, then deletes, and only then leaves the screen.
    fn handle_keypad_back(&mut self) {
        let position = match &self.screen {
            Screen::ChatView {
                pending_hex,
                cursor,
                ..
            } => (pending_hex.is_some(), *cursor),
            Screen::NameEntry {
                pending_hex,
                cursor,
                ..
            } => (pending_hex.is_some(), *cursor),
            Screen::SecretEntry {
                pending_hex,
                cursor,
                ..
            } => (pending_hex.is_some(), *cursor),
            _ => {
                self.handle_key(Key::Back);
                return;
            }
        };
        let (pending_hex, cursor) = position;
        if pending_hex {
            match &mut self.screen {
                Screen::ChatView { pending_hex, .. }
                | Screen::NameEntry { pending_hex, .. }
                | Screen::SecretEntry { pending_hex, .. } => *pending_hex = None,
                _ => {}
            }
        } else if cursor > 0 {
            self.handle_key(Key::Backspace);
        } else {
            self.handle_key(Key::Back);
        }
    }
}
