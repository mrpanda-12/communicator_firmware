//! Application storage facade backed by the dedicated internal-flash partition.

use alloc::{format, string::String};
use sequential_storage::map::{SerializationError, Value};

use crate::{
    hardware::flash::{FlashStore, StoreError},
    storage::{
        contact::Contact,
        message::{MessagePage, StoredMessage},
    },
};

const DEVICE_ID_KEY: &str = "device_id";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeviceIdRecord([u8; 16]);

impl<'a> Value<'a> for DeviceIdRecord {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        let target = buffer
            .get_mut(..16)
            .ok_or(SerializationError::BufferTooSmall)?;
        target.copy_from_slice(&self.0);
        Ok(16)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        let source = buffer.get(..16).ok_or(SerializationError::BufferTooSmall)?;
        let mut id = [0; 16];
        id.copy_from_slice(source);
        Ok((Self(id), 16))
    }
}

pub struct StorageManager<'flash> {
    flash: FlashStore<'flash>,
}

impl<'flash> StorageManager<'flash> {
    pub fn from_flash(flash: FlashStore<'flash>) -> Self {
        Self { flash }
    }

    pub async fn set_setting<T>(
        &mut self,
        key: &str,
        value: &T,
        buffer: &mut [u8],
    ) -> Result<(), String>
    where
        for<'a> T: Value<'a>,
    {
        self.flash
            .set(key, value, buffer)
            .await
            .map_err(store_error)
    }

    pub async fn get_setting<T>(
        &mut self,
        key: &str,
        buffer: &mut [u8],
    ) -> Result<Option<T>, String>
    where
        for<'a> T: Value<'a>,
    {
        self.flash.get(key, buffer).await.map_err(store_error)
    }

    pub async fn load_device_id(&mut self, buffer: &mut [u8]) -> Result<Option<[u8; 16]>, String> {
        Ok(self
            .flash
            .get::<DeviceIdRecord>(DEVICE_ID_KEY, buffer)
            .await
            .map_err(store_error)?
            .map(|stored| stored.0))
    }

    pub async fn save_device_id(&mut self, id: &[u8; 16], buffer: &mut [u8]) -> Result<(), String> {
        self.flash
            .set(DEVICE_ID_KEY, &DeviceIdRecord(*id), buffer)
            .await
            .map_err(store_error)
    }

    pub async fn regenerate_device_id<F>(
        &mut self,
        buffer: &mut [u8],
        mut fill_random: F,
    ) -> Result<[u8; 16], String>
    where
        F: FnMut(&mut [u8; 16]) -> Result<(), String>,
    {
        let previous = self.load_device_id(buffer).await?;
        let mut candidate = [0; 16];
        for _ in 0..8 {
            fill_random(&mut candidate)?;
            if Some(candidate) != previous {
                self.save_device_id(&candidate, buffer).await?;
                return Ok(candidate);
            }
        }
        Err(String::from(
            "TRNG repeatedly returned the current device ID",
        ))
    }

    pub async fn save_contact(
        &mut self,
        contact: &Contact,
        buffer: &mut [u8],
    ) -> Result<(), String> {
        self.flash
            .write_contact(contact, buffer)
            .await
            .map_err(store_error)
    }

    pub async fn load_contact(
        &mut self,
        device_id: &[u8; 16],
        buffer: &mut [u8],
    ) -> Result<Option<Contact>, String> {
        self.flash
            .read_contact(device_id, buffer)
            .await
            .map_err(store_error)
    }

    pub async fn append_messages(
        &mut self,
        device_id: &[u8; 16],
        messages: &[StoredMessage],
        buffer: &mut [u8],
    ) -> Result<(), String> {
        self.flash
            .append_messages(device_id, messages, buffer)
            .await
            .map_err(store_error)
    }

    pub async fn append_broadcast_messages(
        &mut self,
        messages: &[StoredMessage],
        buffer: &mut [u8],
    ) -> Result<(), String> {
        self.flash
            .append_messages(&[0xff; 16], messages, buffer)
            .await
            .map_err(store_error)
    }

    pub async fn load_messages_page(
        &mut self,
        device_id: &[u8; 16],
        offset: u32,
        max_messages: usize,
        buffer: &mut [u8],
    ) -> Result<MessagePage, String> {
        self.flash
            .load_messages_page(device_id, offset, max_messages, buffer)
            .await
            .map_err(store_error)
    }

    pub async fn load_broadcast_messages_page(
        &mut self,
        offset: u32,
        max_messages: usize,
        buffer: &mut [u8],
    ) -> Result<MessagePage, String> {
        self.flash
            .load_messages_page(&[0xff; 16], offset, max_messages, buffer)
            .await
            .map_err(store_error)
    }
}

fn store_error(error: StoreError) -> String {
    format!("flash storage failed: {:?}", error)
}
