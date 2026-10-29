use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use crate::{packet::packet_data, storage::storage_manager::StorageManager};

/// Scratch buffer large enough for the largest stored record.
pub const FLASH_BUFFER_SIZE: usize = 640;

/// Everything one flash operation needs, guarded as a single unit.
pub struct FlashSession<'d> {
    pub storage: StorageManager<'d>,
    pub buffer: [u8; FLASH_BUFFER_SIZE],
}

/// Share one flash device between the receive path, the UI and persistence.
pub struct Flash {
    session: Mutex<CriticalSectionRawMutex, FlashSession<'static>>,
}

impl Flash {
    pub fn new(session: FlashSession<'static>) -> Self {
        Self {
            session: Mutex::new(session),
        }
    }

    /// Wait for exclusive access; drop the guard before locking again.
    pub async fn lock(&self) -> MutexGuard<'_, CriticalSectionRawMutex, FlashSession<'static>> {
        self.session.lock().await
    }

    /// Run one flash operation with the session and its scratch buffer.
    ///
    /// Prefer this over `lock` when the operation is a storage call that needs
    /// `StorageManager` and its buffer together; it cannot leak the guard past
    /// the await and so cannot deadlock a second lock.
    pub async fn with<R>(&self, operation: impl AsyncFnOnce(&mut FlashSession<'static>) -> R) -> R {
        let mut guard = self.session.lock().await;
        operation(&mut guard).await
    }

    /// Persist the advanced counter, then hand out `count` consecutive packet IDs.
    ///
    /// The write happens before the IDs leave the allocator, so a reboot resumes
    /// at the first unused ID: an ID that reached the air is never replayed, and
    /// only a reservation interrupted before it was sent can be skipped. Holding
    /// the session lock for read, write, and advance keeps concurrent producers
    /// from reserving the same range.
    pub async fn reserve_packet_ids(&self, count: u16) -> u32 {
        self.with(async |session| {
            let first = packet_data::current_packet_id();
            let next = first.wrapping_add(u32::from(count));
            if session
                .storage
                .save_packet_ids(next, first, &mut session.buffer)
                .await
                .is_err()
            {
                defmt::warn!("Could not persist the next packet ID");
            }
            packet_data::set_local_packet_ids(next, first);
            first
        })
        .await
    }
}
