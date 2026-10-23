use core::ops::Range;

use embassy_embedded_hal::adapter::BlockingAsync;
use esp_storage::{Flash, FlashStorage, FlashStorageError};
use sequential_storage::{
    cache::{Cache, Uncached},
    map::{MapConfig, MapStorage, Value},
};

use crate::storage::{contact::Contact, message::StoredMessage, settings::StorageKey};
use alloc::vec::Vec;

type StorageCache = Cache<Uncached, Uncached, Uncached, StorageKey>;
type AsyncFlash<'d> = BlockingAsync<FlashStorage<'d>>;

#[derive(Debug)]
pub enum StoreError {
    InvalidKey,
    Storage(sequential_storage::Error<FlashStorageError>),
}

pub struct FlashStore<'d> {
    storage: MapStorage<StorageKey, AsyncFlash<'d>, StorageCache>,
}

impl<'d> FlashStore<'d> {
    pub fn new(
        flash: Flash<'d>,
        flash_range: Range<u32>,
    ) -> Result<Self, sequential_storage::map::MapConfigError> {
        let config = MapConfig::try_new(flash_range)?;
        let flash = BlockingAsync::new(FlashStorage::new(flash));
        let storage = MapStorage::new(flash, config, Cache::new_uncached());

        Ok(Self { storage })
    }

    pub async fn get<'a, V: Value<'a>>(
        &mut self,
        name: &str,
        data_buffer: &'a mut [u8],
    ) -> Result<Option<V>, StoreError> {
        let key = StorageKey::setting(name).ok_or(StoreError::InvalidKey)?;
        self.storage
            .fetch_item(data_buffer, &key)
            .await
            .map_err(StoreError::Storage)
    }

    pub async fn set<'a, V: Value<'a>>(
        &mut self,
        name: &str,
        value: &V,
        data_buffer: &'a mut [u8],
    ) -> Result<(), StoreError> {
        let key = StorageKey::setting(name).ok_or(StoreError::InvalidKey)?;
        self.storage
            .store_item(data_buffer, &key, value)
            .await
            .map_err(StoreError::Storage)
    }

    pub async fn read_contact(
        &mut self,
        device_id: &[u8; 16],
        data_buffer: &mut [u8],
    ) -> Result<Option<Contact>, StoreError> {
        let key = StorageKey::contact(device_id);
        self.storage
            .fetch_item(data_buffer, &key)
            .await
            .map_err(StoreError::Storage)
    }

    pub async fn write_contact(
        &mut self,
        contact: &Contact,
        data_buffer: &mut [u8],
    ) -> Result<(), StoreError> {
        let key = StorageKey::contact(&contact.device_id);
        self.storage
            .store_item(data_buffer, &key, contact)
            .await
            .map_err(StoreError::Storage)
    }

    pub async fn remove_contact(
        &mut self,
        device_id: &[u8; 16],
        data_buffer: &mut [u8],
    ) -> Result<(), StoreError> {
        let key = StorageKey::contact(device_id);
        self.storage
            .remove_item(data_buffer, &key)
            .await
            .map_err(StoreError::Storage)
    }

    pub async fn get_contact(
        &mut self,
        device_id: &[u8; 16],
        data_buffer: &mut [u8],
    ) -> Result<Option<Contact>, StoreError> {
        self.read_contact(device_id, data_buffer).await
    }

    pub async fn save_messages(
        &mut self,
        device_id: &[u8; 16],
        messages: &Vec<StoredMessage>,
        data_buffer: &mut [u8],
    ) -> Result<(), StoreError> {
        use crate::storage::message::StoredMessageList;
        let key = StorageKey::message_list(device_id);
        self.storage
            .store_item(data_buffer, &key, &StoredMessageList(messages.clone()))
            .await
            .map_err(StoreError::Storage)
    }

    pub async fn load_messages(
        &mut self,
        device_id: &[u8; 16],
        data_buffer: &mut [u8],
    ) -> Result<Vec<StoredMessage>, StoreError> {
        use crate::storage::message::StoredMessageList;
        let key = StorageKey::message_list(device_id);
        match self.storage.fetch_item(data_buffer, &key).await {
            Ok(Some(StoredMessageList(msgs))) => Ok(msgs),
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(StoreError::Storage(e)),
        }
    }

    pub async fn save_broadcast_messages(
        &mut self,
        messages: &Vec<StoredMessage>,
        data_buffer: &mut [u8],
    ) -> Result<(), StoreError> {
        use crate::storage::message::StoredMessageList;
        let key = StorageKey::broadcast_messages();
        self.storage
            .store_item(data_buffer, &key, &StoredMessageList(messages.clone()))
            .await
            .map_err(StoreError::Storage)
    }

    pub async fn load_broadcast_messages(
        &mut self,
        data_buffer: &mut [u8],
    ) -> Result<Vec<StoredMessage>, StoreError> {
        use crate::storage::message::StoredMessageList;
        let key = StorageKey::broadcast_messages();
        match self.storage.fetch_item(data_buffer, &key).await {
            Ok(Some(StoredMessageList(msgs))) => Ok(msgs),
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(StoreError::Storage(e)),
        }
    }
}
