//! Timed key-input demo for checking every current display screen without controls.

use embassy_time::{Duration, Instant, Timer};

use super::state::Key;

const STEP_DELAY: Duration = Duration::from_millis(1_200);
// 1/ Visit Contacts discovery and every settings option before opening the chats.
const DEMO_KEYS: &[Key] = &[
    // Open Contacts and its passive device search.
    Key::Down,
    Key::Enter,
    Key::Enter,
    Key::Back,
    Key::Back,
    // Open Settings from the main menu.
    Key::Down,
    Key::Enter,
    // Device: inspect the device info page and reset preview.
    Key::Enter,
    Key::Enter,
    Key::Back,
    Key::Down,
    Key::Enter,
    Key::Back,
    Key::Back,
    // Frequency: inspect current frequency and the change-frequency preview.
    Key::Down,
    Key::Enter,
    Key::Enter,
    Key::Back,
    Key::Down,
    Key::Enter,
    Key::Back,
    Key::Back,
    // GPS: inspect fix status, coordinates, and UTC time.
    Key::Down,
    Key::Enter,
    Key::Enter,
    Key::Back,
    Key::Down,
    Key::Enter,
    Key::Back,
    Key::Down,
    Key::Enter,
    Key::Back,
    Key::Back,
    // Return to chats and exercise text entry without transmitting the draft.
    Key::Back,
    Key::Up,
    Key::Up,
    Key::Enter,
    Key::Enter,
    Key::Character('D'),
    Key::Character('E'),
    Key::Character('M'),
    Key::Character('O'),
    Key::Left,
    Key::Backspace,
    Key::Enter,
    Key::Enter,
    Key::Back,
    Key::Back,
];

/// 1/ Replay button presses through the real UI state machine, one at a time.
pub struct FakeNavigator {
    next_step: usize,
    next_deadline: Option<Instant>,
}

impl FakeNavigator {
    /// 2/ Start the demo at the first main-menu navigation event.
    pub const fn new() -> Self {
        Self {
            next_step: 0,
            next_deadline: None,
        }
    }

    /// 3/ Wait for the demo interval and return the next simulated key press.
    pub async fn next_key(&mut self) -> Key {
        let deadline = match self.next_deadline {
            Some(deadline) => deadline,
            None => {
                let deadline = Instant::now() + STEP_DELAY;
                self.next_deadline = Some(deadline);
                deadline
            }
        };
        Timer::at(deadline).await;
        self.next_deadline = Some(Instant::now() + STEP_DELAY);

        let key = DEMO_KEYS[self.next_step];
        self.next_step = (self.next_step + 1) % DEMO_KEYS.len();
        key
    }
}

impl Default for FakeNavigator {
    fn default() -> Self {
        Self::new()
    }
}
