//! Host-side test harness for the firmware's hardware-independent modules.
//!
//! The firmware crate is `#![no_std]` and targets `xtensa-esp32s3`, so its
//! `#[cfg(test)]` code cannot run here. This crate pulls the pure modules out
//! of `src/bin` with `#[path]` so the protocol, storage encoding, radio config,
//! contact book, notification queue, outbox, date math, and hex formatting can
//! be tested on the host.
//!
//! Run with `cargo test` inside `host-tests/`.

pub mod network;
pub mod packet;
pub mod radio;
pub mod storage;
pub mod ui;

#[path = "../../src/bin/calendar.rs"]
pub mod calendar;

#[path = "../../src/bin/hex.rs"]
pub mod hex;

#[path = "../../src/bin/notify/mod.rs"]
pub mod notify;

/// Mirror of `src/bin/crypto/mod.rs`. The real module needs `esp_hal` for the
/// RNG, so the key length is repeated here and pinned by
/// `tests::crypto_key_size_matches_firmware`.
pub mod crypto {
    pub const KEY_SIZE: usize = 32;
}

/// Stand-in for the firmware's fatal-error screen; tests must never reach it.
pub fn fatal(reason: &str) -> ! {
    panic!("fatal: {reason}")
}

/// Minimal logger so `defmt` logging in the outbox links on the host.
#[defmt::global_logger]
struct TestLogger;

unsafe impl defmt::Logger for TestLogger {
    fn acquire() {}
    unsafe fn release() {}
    unsafe fn flush() {}
    unsafe fn write(_bytes: &[u8]) {}
}

defmt::timestamp!("{=u32}", 0);

#[cfg(test)]
mod tests;
