//! Device IDs whose traffic this device drops.
//!
//! Every blocked ID is its own flash record, so the list has no fixed length:
//! a peer can be blocked without ever becoming a contact, blocking never
//! rewrites the whole list, and the count is bounded only by the partition.
//! A RAM copy mirrors those records because the receive path checks it on
//! every frame addressed to this device.

use core::cell::RefCell;

use alloc::vec::Vec;
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use sequential_storage::map::{SerializationError, Value};

/// One stored block record: the device ID it blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockedDevice(pub [u8; 16]);

impl<'a> Value<'a> for BlockedDevice {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        let target = buffer
            .get_mut(..16)
            .ok_or(SerializationError::BufferTooSmall)?;
        target.copy_from_slice(&self.0);
        Ok(16)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        let source = buffer.get(..16).ok_or(SerializationError::BufferTooSmall)?;
        let mut device_id = [0; 16];
        device_id.copy_from_slice(source);
        Ok((Self(device_id), 16))
    }
}

/// Device IDs blocked on this device, in the order they were stored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockList {
    ids: Vec<[u8; 16]>,
}

impl BlockList {
    pub const fn new() -> Self {
        Self { ids: Vec::new() }
    }

    pub fn from_ids(ids: Vec<[u8; 16]>) -> Self {
        Self { ids }
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn ids(&self) -> &[[u8; 16]] {
        self.ids.as_slice()
    }

    pub fn get(&self, index: usize) -> Option<&[u8; 16]> {
        self.ids.get(index)
    }

    pub fn contains(&self, device_id: &[u8; 16]) -> bool {
        self.ids.contains(device_id)
    }

    /// Add or remove one ID; true when the list changed.
    ///
    /// Setting the state an ID already has changes nothing.
    pub fn set(&mut self, device_id: &[u8; 16], blocked: bool) -> bool {
        match (blocked, self.ids.iter().position(|id| id == device_id)) {
            (true, None) => {
                self.ids.push(*device_id);
                true
            }
            (false, Some(index)) => {
                self.ids.remove(index);
                true
            }
            _ => false,
        }
    }
}

/// RAM copy of the persisted records: seeded at boot, written through on change.
static BLOCKED: Mutex<CriticalSectionRawMutex, RefCell<BlockList>> =
    Mutex::new(RefCell::new(BlockList::new()));

/// True when this device ID is on the block list.
pub fn is_blocked(device_id: &[u8; 16]) -> bool {
    BLOCKED.lock(|list| list.borrow().contains(device_id))
}

/// Copy the current list, for the screen that lists and clears entries.
pub fn snapshot() -> BlockList {
    BLOCKED.lock(|list| list.borrow().clone())
}

/// Add or remove one ID in the RAM copy; true when it changed.
pub fn set(device_id: [u8; 16], blocked: bool) -> bool {
    BLOCKED.lock(|list| list.borrow_mut().set(&device_id, blocked))
}

/// Replace the RAM copy with the list just read from flash.
pub fn replace(list: BlockList) {
    BLOCKED.lock(|current| *current.borrow_mut() = list);
}
