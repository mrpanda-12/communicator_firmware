use esp_hal::rng::Trng;

use crate::storage::flash::{FlashStore, StoreError};

const DEVICE_ID_SETTING: &str = "device_id";

pub type DeviceId = [u8; 16];

// Loads the local device ID without generating a replacement.
pub async fn load_device_id(
    store: &mut FlashStore<'_>,
    data_buffer: &mut [u8],
) -> Result<Option<DeviceId>, StoreError> {
    store.get(DEVICE_ID_SETTING, data_buffer).await
}

// Generates and saves a new ID; keep the TRNG entropy source active during this call.
pub async fn regenerate_device_id(
    store: &mut FlashStore<'_>,
    trng: &Trng,
    data_buffer: &mut [u8],
) -> Result<DeviceId, StoreError> {
    let current_id = load_device_id(store, data_buffer).await?;
    let mut new_id = [0; 16];

    loop {
        trng.read(&mut new_id);
        if current_id != Some(new_id) {
            break;
        }
    }

    store.set(DEVICE_ID_SETTING, &new_id, data_buffer).await?;
    Ok(new_id)
}
