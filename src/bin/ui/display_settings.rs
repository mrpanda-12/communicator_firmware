//! Persistent user settings backed by flash.

use crate::storage::storage_manager::StorageManager;

pub const DEFAULT_BRIGHTNESS: u8 = 80;
pub const BRIGHTNESS_SETTING: &str = "display_brightness";

/// Displayed time zone: minutes east of UTC.
/// Range covers UTC-12:00 through UTC+14:00.
pub const DEFAULT_TIMEZONE_OFFSET: i32 = 0;
pub const TIMEZONE_SETTING: &str = "timezone_offset_minutes";
pub const MIN_TIMEZONE_OFFSET: i32 = -12 * 60;
pub const MAX_TIMEZONE_OFFSET: i32 = 14 * 60;
/// One adjust step: 30 minutes.
pub const TIMEZONE_STEP: i32 = 30;

/// 1/ Load the saved backlight percentage and persist its default on first boot.
pub async fn load_brightness(storage: &mut StorageManager<'static>, flash_buffer: &mut [u8]) -> u8 {
    match storage
        .get_setting::<u8>(BRIGHTNESS_SETTING, flash_buffer)
        .await
    {
        Ok(Some(brightness)) if brightness <= 100 => brightness,
        Ok(Some(_)) => {
            defmt::warn!("Stored display brightness is outside 0-100; using default");
            DEFAULT_BRIGHTNESS
        }
        Ok(None) => {
            if storage
                .set_setting(BRIGHTNESS_SETTING, &DEFAULT_BRIGHTNESS, flash_buffer)
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

/// 2/ Load the saved UTC offset in minutes, clamped to the supported range.
pub async fn load_timezone_offset(
    storage: &mut StorageManager<'static>,
    flash_buffer: &mut [u8],
) -> i32 {
    match storage
        .get_setting::<i32>(TIMEZONE_SETTING, flash_buffer)
        .await
    {
        Ok(Some(offset)) if (MIN_TIMEZONE_OFFSET..=MAX_TIMEZONE_OFFSET).contains(&offset) => offset,
        Ok(Some(_)) => {
            defmt::warn!("Stored UTC offset is outside -12:00..+14:00; using UTC");
            DEFAULT_TIMEZONE_OFFSET
        }
        Ok(None) => {
            if storage
                .set_setting(TIMEZONE_SETTING, &DEFAULT_TIMEZONE_OFFSET, flash_buffer)
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

/// 3/ Persist a new UTC offset in minutes.
pub async fn save_timezone_offset(
    storage: &mut StorageManager<'static>,
    offset: i32,
    flash_buffer: &mut [u8],
) -> Result<(), ()> {
    storage
        .set_setting(TIMEZONE_SETTING, &offset, flash_buffer)
        .await
        .map(|_| ())
        .map_err(|_| ())
}
