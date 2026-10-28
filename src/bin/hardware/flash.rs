use core::ops::Range;

use alloc::vec::Vec;
use embassy_embedded_hal::adapter::BlockingAsync;
use esp_storage::{Flash, FlashStorage, FlashStorageError};
use sequential_storage::{
    cache::{Cache, Uncached},
    map::{MapConfig, MapStorage, Value},
};

use crate::storage::{
    contact::Contact,
    message::StoredMessage,
    settings::StorageKey,
};

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
    pub async fn get<V: for<'a> Value<'a>>(
        &mut self,
        name: &str,
        data_buffer: &mut [u8],
    ) -> Result<Option<V>, StoreError> {
        let key = StorageKey::setting(name).ok_or(StoreError::InvalidKey)?;
        self.read_item::<V>(&key, data_buffer).await
    }

    /// 3/ Save or replace a typed setting using the caller's reusable scratch buffer.
    ///
    /// The setting name must be nonempty and no longer than 31 UTF-8 bytes.
    pub async fn set<V: for<'a> Value<'a>>(
        &mut self,
        name: &str,
        value: &V,
        data_buffer: &mut [u8],
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
        self.read_item::<Contact>(&key, data_buffer).await
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

    /// 7/ Append records under per-conversation keys that index monotonically.
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
            .read_item::<u32>(&count_key, buffer)
            .await?
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

    /// 8/ Read the newest records of one conversation, oldest first.
    pub async fn load_recent_messages(
        &mut self,
        device_id: &[u8; 16],
        max_messages: usize,
        buffer: &mut [u8],
    ) -> Result<Vec<StoredMessage>, StoreError> {
        if max_messages == 0 {
            return Err(StoreError::InvalidKey);
        }
        let count = self
            .read_item::<u32>(&StorageKey::message_count(device_id), buffer)
            .await?
            .unwrap_or(0);
        let start = count.saturating_sub(max_messages.min(u32::MAX as usize) as u32);
        let mut messages: Vec<StoredMessage> = Vec::with_capacity((count - start).min(max_messages as u32) as usize);
        for index in start..count {
            let key = StorageKey::message(device_id, index);
            match self.read_item::<StoredMessage>(&key, buffer).await {
                Ok(Some(message)) => messages.push(message),
                Ok(None) => return Err(StoreError::MissingMessageRecord),
                Err(error) => return Err(error),
            }
        }
        Ok(messages)
    }

    /// 9/ Mark every sent record with this message ID as acknowledged and count how many moved.
    pub async fn mark_acknowledged(
        &mut self,
        device_id: &[u8; 16],
        message_id: u16,
        buffer: &mut [u8],
    ) -> Result<usize, StoreError> {
        let count = self
            .read_item::<u32>(&StorageKey::message_count(device_id), buffer)
            .await?
            .unwrap_or(0);
        let mut marked = 0;
        for index in 0..count {
            let key = StorageKey::message(device_id, index);
            let Some(mut message) = self.read_item::<StoredMessage>(&key, buffer).await? else {
                continue;
            };
            if message.message_id != message_id || message.acknowledged {
                continue;
            }
            message.acknowledged = true;
            self.storage
                .store_item(buffer, &key, &message)
                .await
                .map_err(StoreError::Storage)?;
            marked += 1;
        }
        Ok(marked)
    }

    /// 10/ Erase every key of one conversation's history.
    ///
    /// This is a full underlying-flash scan-and-delete; a conversation occupies
    /// few records on this device, and count entries are erased alongside them.
    pub async fn delete_conversation(
        &mut self,
        device_id: &[u8; 16],
        buffer: &mut [u8],
    ) -> Result<(), StoreError> {
        let count = self
            .read_item::<u32>(&StorageKey::message_count(device_id), buffer)
            .await?
            .unwrap_or(0);
        for index in 0..count {
            self.storage
                .remove_item(&mut buffer[..], &StorageKey::message(device_id, index))
                .await
                .map_err(StoreError::Storage)?;
        }
        self.storage
            .remove_item(&mut buffer[..], &StorageKey::message_count(device_id))
            .await
            .map_err(StoreError::Storage)?;
        Ok(())
    }

    /// 11/ Erase the history of every conversation, including orphaned ones.
    ///
    /// The scan removes every `message`/`message_count` key found under any
    /// device ID, not only conversations the contact list still knows about.
    pub async fn delete_all_conversations(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(), StoreError> {
        let mut keys: Vec<StorageKey> = Vec::new();
        {
            let mut iterator = self
                .storage
                .fetch_all_items(buffer)
                .await
                .map_err(StoreError::Storage)?;
            while let Some((key, _)) = iterator
                .next::<&[u8]>(buffer)
                .await
                .map_err(StoreError::Storage)?
            {
                if key.is_conversation_message() {
                    let _ = keys.push(key);
                }
            }
        }
        for key in keys {
            self.storage
                .remove_item(&mut buffer[..], &key)
                .await
                .map_err(StoreError::Storage)?;
        }
        Ok(())
    }

    /// Shared typed read over the storage map.
    async fn read_item<'a, V: Value<'a>>(
        &mut self,
        key: &StorageKey,
        data_buffer: &'a mut [u8],
    ) -> Result<Option<V>, StoreError> {
        self.storage
            .fetch_item(data_buffer, key)
            .await
            .map_err(StoreError::Storage)
    }
}
