use core::ops::Range;

use embassy_embedded_hal::adapter::BlockingAsync;
use esp_storage::{Flash, FlashStorage, FlashStorageError};
use sequential_storage::{
    cache::{Cache, Uncached},
    map::{MapConfig, MapStorage, Value},
};

use crate::storage::{
    contact::Contact,
    message::{MessagePage, StoredMessage},
    settings::StorageKey,
};
use alloc::vec::Vec;

type StorageCache = Cache<Uncached, Uncached, Uncached, StorageKey>;
type AsyncFlash<'d> = BlockingAsync<FlashStorage<'d>>;

#[derive(Debug)]
pub enum StoreError {
    InvalidKey,
    InvalidContact,
    MessageIndexOverflow,
    MissingMessageRecord,
    Storage(sequential_storage::Error<FlashStorageError>),
}

pub struct FlashStore<'d> {
    storage: MapStorage<StorageKey, AsyncFlash<'d>, StorageCache>,
}

impl<'d> FlashStore<'d> {
    /// 1/ Open the reserved flash partition and prepare its indexed storage map.
    ///
    /// The range must be dedicated to application data, erase-page aligned, and
    /// large enough for the map. This method cannot verify the partition table.
    pub fn new(
        flash: Flash<'d>,
        flash_range: Range<u32>,
    ) -> Result<Self, sequential_storage::map::MapConfigError> {
        let config = MapConfig::try_new(flash_range)?;
        let flash = BlockingAsync::new(FlashStorage::new(flash));
        let storage = MapStorage::new(flash, config, Cache::new_uncached());

        Ok(Self { storage })
    }

    /// 2/ Read a typed setting by name; return `None` when the name has no value.
    ///
    /// Keep the type associated with each setting name stable between firmware
    /// versions because the serialized value has no separate type tag.
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

    /// 3/ Save or replace a typed setting using the caller's reusable scratch buffer.
    ///
    /// The setting name must be nonempty and no longer than 31 UTF-8 bytes.
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

    /// 4/ Find a contact by its 16-byte device ID without loading other contacts.
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

    /// 5/ Load the latest records stored under contact keys for the UI list.
    pub async fn load_contacts(
        &mut self,
        data_buffer: &mut [u8],
    ) -> Result<Vec<Contact>, StoreError> {
        let mut iterator = self
            .storage
            .fetch_all_items(data_buffer)
            .await
            .map_err(StoreError::Storage)?;
        let mut contacts: Vec<Contact> = Vec::new();

        while let Some((key, value)) = iterator
            .next::<&[u8]>(data_buffer)
            .await
            .map_err(StoreError::Storage)?
        {
            if !key.is_contact() {
                continue;
            }

            let (contact, _) =
                Contact::deserialize_from(value).map_err(|_| StoreError::InvalidContact)?;
            if let Some(existing) = contacts
                .iter_mut()
                .find(|existing| existing.device_id == contact.device_id)
            {
                *existing = contact;
            } else {
                contacts.push(contact);
            }
        }

        Ok(contacts)
    }

    /// 6/ Insert or replace a contact, including its serialized pairwise key.
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

    /// 7/ Append only newly received or created records under monotonic per-conversation keys.
    ///
    /// The separate count entry advances only after all records have been
    /// written, so a retry after a partial failure can safely reuse those indices.
    pub async fn append_messages(
        &mut self,
        device_id: &[u8; 16],
        messages: &[StoredMessage],
        buffer: &mut [u8],
    ) -> Result<(), StoreError> {
        let count_key = StorageKey::message_count(device_id);
        let mut count = self
            .storage
            .fetch_item::<u32>(buffer, &count_key)
            .await
            .map_err(StoreError::Storage)?
            .unwrap_or(0);
        for message in messages {
            let key = StorageKey::message(device_id, count);
            self.storage
                .store_item(buffer, &key, message)
                .await
                .map_err(StoreError::Storage)?;
            count = count
                .checked_add(1)
                .ok_or(StoreError::MessageIndexOverflow)?;
        }
        self.storage
            .store_item(buffer, &count_key, &count)
            .await
            .map_err(StoreError::Storage)
    }

    /// 8/ Read one bounded page of records using a record index as the cursor.
    ///
    /// Pass `next_offset` to the next call. Keep the page size small enough for
    /// RAM; this backend's cursor is a record index (SD uses a byte offset).
    pub async fn load_messages_page(
        &mut self,
        device_id: &[u8; 16],
        offset: u32,
        max_messages: usize,
        buffer: &mut [u8],
    ) -> Result<MessagePage, StoreError> {
        if max_messages == 0 {
            return Err(StoreError::InvalidKey);
        }
        let count = self
            .storage
            .fetch_item::<u32>(buffer, &StorageKey::message_count(device_id))
            .await
            .map_err(StoreError::Storage)?
            .unwrap_or(0);
        if offset > count {
            return Err(StoreError::InvalidKey);
        }
        let end = offset
            .saturating_add(max_messages.min(u32::MAX as usize) as u32)
            .min(count);
        let mut messages = Vec::with_capacity((end - offset) as usize);
        for index in offset..end {
            let key = StorageKey::message(device_id, index);
            if let Some(message) = self
                .storage
                .fetch_item::<StoredMessage>(buffer, &key)
                .await
                .map_err(StoreError::Storage)?
            {
                messages.push(message);
            } else {
                return Err(StoreError::MissingMessageRecord);
            }
        }
        Ok(MessagePage {
            messages,
            next_offset: end,
            done: end == count,
        })
    }
}
