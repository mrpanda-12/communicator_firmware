//! Bounded retry queue for outgoing chat messages.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use heapless::Vec;

use crate::packet::packet_data::{MAX_FRAGMENT_COUNT, PacketData};

/// Messages kept in RAM until the peer acknowledges them.
pub const MAX_OUTBOX_SLOTS: usize = 2;
/// Transmissions attempted for one message, including the first one.
const MAX_ATTEMPTS: u8 = 3;
/// Delay between the end of one transmission round and the next retry.
const RETRY_INTERVAL_US: u64 = 20_000_000;

/// Reports a full outbox; the caller keeps the draft instead of dropping it.
#[derive(Debug)]
pub struct OutboxFull;

struct Pending {
    message_id: u32,
    packets: Vec<PacketData, MAX_FRAGMENT_COUNT>,
    next: usize,
    attempts: u8,
    retry_at_us: u64,
    expect_ack: bool,
}

const EMPTY: State = State {
    slots: [const { None }; MAX_OUTBOX_SLOTS],
};

static OUTBOX: Mutex<CriticalSectionRawMutex, RefCell<State>> = Mutex::new(RefCell::new(EMPTY));

struct State {
    slots: [Option<Pending>; MAX_OUTBOX_SLOTS],
}

/// Take ownership of one message's fragments for delivery.
pub fn submit(
    message_id: u32,
    packets: Vec<PacketData, MAX_FRAGMENT_COUNT>,
    expect_ack: bool,
) -> Result<(), OutboxFull> {
    OUTBOX.lock(|cell| {
        let state = &mut *cell.borrow_mut();
        let Some(slot) = state.slots.iter_mut().find(|slot| slot.is_none()) else {
            return Err(OutboxFull);
        };
        *slot = Some(Pending {
            message_id,
            packets,
            next: 0,
            attempts: 0,
            retry_at_us: 0,
            expect_ack,
        });
        Ok(())
    })
}

/// Drop one message once its acknowledgement arrives.
pub fn acknowledge(message_id: u32) {
    OUTBOX.lock(|cell| {
        for slot in cell.borrow_mut().slots.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|pending| pending.message_id == message_id)
            {
                *slot = None;
            }
        }
    });
}

/// Send whatever fits, then schedule or perform the next retry.
///
/// `drained` is true when the radio FIFO is empty, so a retry timer only starts
/// after the previous round has actually left the queue. Returns the number of
/// messages still waiting for delivery.
pub fn pump(now_us: u64, drained: bool, mut send: impl FnMut(&PacketData) -> bool) -> usize {
    OUTBOX.lock(|cell| {
        let state = &mut *cell.borrow_mut();
        let mut waiting = 0;

        for slot in state.slots.iter_mut() {
            let Some(pending) = slot.as_mut() else {
                continue;
            };

            if pending.next < pending.packets.len() {
                while pending.next < pending.packets.len() {
                    if !send(&pending.packets[pending.next]) {
                        break;
                    }
                    pending.next += 1;
                }
                if pending.next == pending.packets.len() && drained && pending.retry_at_us == 0 {
                    pending.attempts += 1;
                    pending.retry_at_us = now_us + RETRY_INTERVAL_US;
                    // Broadcast and plaintext sends have no acknowledgement.
                    if !pending.expect_ack {
                        *slot = None;
                        continue;
                    }
                }
            } else if pending.retry_at_us != 0 && now_us >= pending.retry_at_us {
                if pending.attempts >= MAX_ATTEMPTS {
                    defmt::warn!(
                        "Giving up on message {:x} after {} attempts",
                        pending.message_id,
                        pending.attempts
                    );
                    *slot = None;
                    continue;
                }
                pending.next = 0;
                pending.retry_at_us = 0;
            }

            waiting += 1;
        }

        waiting
    })
}
