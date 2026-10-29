use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use crate::storage::storage_manager::StorageManager;

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
}
