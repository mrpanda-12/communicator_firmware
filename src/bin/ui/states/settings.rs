//! User settings the UI owns, loaded from flash at boot.
//!
//! Each setting's flash key, value type, default, and accepted range are
//! defined together here, so no call site can read a key back as the wrong
//! type or drift from the stored defaults.

use crate::storage::storage_manager::StorageManager;

pub const BRIGHTNESS_SETTING: &str = "display_brightness";
pub const DEFAULT_BRIGHTNESS: u8 = 80;
/// Backlight percentage the display accepts; anything above is a bad record.
pub const MAX_BRIGHTNESS: u8 = 100;

/// Displayed time zone: minutes east of UTC.
/// Range covers UTC-12:00 through UTC+14:00.
pub const TIMEZONE_SETTING: &str = "timezone_offset_minutes";
pub const DEFAULT_TIMEZONE_OFFSET: i32 = 0;
pub const MIN_TIMEZONE_OFFSET: i32 = -12 * 60;
pub const MAX_TIMEZONE_OFFSET: i32 = 14 * 60;
/// One adjust step: 30 minutes.
pub const TIMEZONE_STEP: i32 = 30;

/// Every persisted UI setting, typed as it is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Backlight percentage driven onto GPIO7.
    pub display_brightness: u8,
    /// Minutes east of UTC added to the top-bar clock.
    pub timezone_offset_minutes: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            display_brightness: DEFAULT_BRIGHTNESS,
            timezone_offset_minutes: DEFAULT_TIMEZONE_OFFSET,
        }
    }
}

impl Settings {
    /// Load every setting, writing defaults on first boot and falling back to
    /// the defaults for records outside their accepted range.
    pub async fn load(storage: &mut StorageManager<'static>, flash_buffer: &mut [u8]) -> Self {
        Self {
            display_brightness: load_brightness(storage, flash_buffer).await,
            timezone_offset_minutes: load_timezone_offset(storage, flash_buffer).await,
        }
    }

    /// Persist a new UTC offset in minutes.
    pub async fn save_timezone_offset(
        storage: &mut StorageManager<'static>,
        offset: i32,
        flash_buffer: &mut [u8],
    ) -> Result<(), ()> {
        storage
            .save_setting(TIMEZONE_SETTING, &offset, flash_buffer)
            .await
            .map(|_| ())
            .map_err(|_| ())
    }

    /// Nudge the offset by one step, clamped to the supported range.
    /// Returns the new value only when it actually changed.
    pub fn adjust_timezone_offset(&mut self, delta_minutes: i32) -> Option<i32> {
        let next = (self.timezone_offset_minutes + delta_minutes)
            .clamp(MIN_TIMEZONE_OFFSET, MAX_TIMEZONE_OFFSET);
        if next == self.timezone_offset_minutes {
            return None;
        }
        self.timezone_offset_minutes = next;
        Some(next)
    }
}

/// Load the saved backlight percentage and persist its default on first boot.
async fn load_brightness(storage: &mut StorageManager<'static>, flash_buffer: &mut [u8]) -> u8 {
    match storage
        .load_setting::<u8>(BRIGHTNESS_SETTING, flash_buffer)
        .await
    {
        Ok(Some(brightness)) if brightness <= MAX_BRIGHTNESS => brightness,
        Ok(Some(_)) => {
            defmt::warn!("Stored display brightness is outside 0-100; using default");
            DEFAULT_BRIGHTNESS
        }
        Ok(None) => {
            if storage
                .save_setting(BRIGHTNESS_SETTING, &DEFAULT_BRIGHTNESS, flash_buffer)
                .await
                .is_ok()
            {
                defmt::info!("Saved default display brightness");
            } else {
                defmt::warn!("Could not save default display brightness");
            }
            DEFAULT_BRIGHTNESS
        }
        Err(_) => {
            defmt::warn!("Could not load display brightness; using default");
            DEFAULT_BRIGHTNESS
        }
    }
}

/// Load the saved UTC offset in minutes, rejecting values outside the range.
async fn load_timezone_offset(
    storage: &mut StorageManager<'static>,
    flash_buffer: &mut [u8],
) -> i32 {
    match storage
        .load_setting::<i32>(TIMEZONE_SETTING, flash_buffer)
        .await
    {
        Ok(Some(offset)) if (MIN_TIMEZONE_OFFSET..=MAX_TIMEZONE_OFFSET).contains(&offset) => offset,
        Ok(Some(_)) => {
            defmt::warn!("Stored UTC offset is outside -12:00..+14:00; using UTC");
            DEFAULT_TIMEZONE_OFFSET
        }
        Ok(None) => {
            if storage
                .save_setting(TIMEZONE_SETTING, &DEFAULT_TIMEZONE_OFFSET, flash_buffer)
                .await
                .is_ok()
            {
                defmt::info!("Saved default UTC offset");
            }
            DEFAULT_TIMEZONE_OFFSET
        }
        Err(_) => {
            defmt::warn!("Could not load UTC offset; using UTC");
            DEFAULT_TIMEZONE_OFFSET
        }
    }
}
