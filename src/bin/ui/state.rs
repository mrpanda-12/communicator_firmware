use heapless::String;

use super::contact::ContactBook;
pub use super::contact::ConversationKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    MainMenu {
        selected: usize,
    },
    ChatsList {
        selected: usize,
    },
    ChatView {
        conversation: ConversationKind,
        scroll: usize,
        input: String<200>,
        cursor: usize,
        menu_open: bool,
    },
    Settings {
        selected: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Back,
    Character(char),
    Backspace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub screen: Screen,
    pub contacts: ContactBook,
}

impl UiState {
    /// 1/ Start at the application menu with chats selected.
    pub fn new() -> Self {
        Self {
            screen: Screen::MainMenu { selected: 0 },
            contacts: ContactBook::new(),
        }
    }

    /// 2/ Apply a logical key event; the input hardware can map buttons to this API.
    pub fn handle_key(&mut self, key: Key) {
        match &mut self.screen {
            Screen::MainMenu { selected } => match key {
                Key::Up | Key::Down => *selected = (*selected + 1) % 2,
                Key::Enter if *selected == 0 => self.screen = Screen::ChatsList { selected: 0 },
                Key::Enter => self.screen = Screen::Settings { selected: 0 },
                _ => {}
            },
            Screen::ChatsList { selected } => match key {
                Key::Back => self.screen = Screen::MainMenu { selected: 0 },
                Key::Up if *selected > 0 => *selected -= 1,
                Key::Down if *selected + 1 < self.contacts.entries().len() => *selected += 1,
                Key::Enter if self.contacts.get(*selected).is_some() => {
                    let conversation = self.contacts.get(*selected).unwrap().kind;
                    self.screen = Screen::ChatView {
                        conversation,
                        scroll: 0,
                        input: String::new(),
                        cursor: 0,
                        menu_open: false,
                    };
                }
                Key::Enter => {}
                _ => {}
            },
            Screen::ChatView {
                conversation: _,
                scroll,
                input,
                cursor,
                menu_open,
            } => match key {
                Key::Back => self.screen = Screen::ChatsList { selected: 0 },
                Key::Up => *scroll = scroll.saturating_add(1),
                Key::Down => *scroll = scroll.saturating_sub(1),
                Key::Left => *cursor = cursor.saturating_sub(1),
                Key::Right => *cursor = (*cursor + 1).min(input.len()),
                Key::Enter => *menu_open = !*menu_open,
                Key::Backspace if *cursor > 0 => {
                    input.remove(*cursor - 1);
                    *cursor -= 1;
                }
                Key::Character(ch) if ch.is_ascii() && !ch.is_control() => {
                    if input.push(ch).is_ok() {
                        *cursor = input.len();
                    }
                }
                _ => {}
            },
            Screen::Settings { selected } => match key {
                Key::Back => self.screen = Screen::MainMenu { selected: 1 },
                Key::Up | Key::Down => *selected = (*selected + 1) % 4,
                _ => {}
            },
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}
