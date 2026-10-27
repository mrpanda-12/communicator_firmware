//! Persistent settings used by the display UI.

use crate::storage::storage_manager::StorageManager;

pub const DEFAULT_BRIGHTNESS: u8 = 80;
pub const BRIGHTNESS_SETTING: &str = "display_brightness";

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
