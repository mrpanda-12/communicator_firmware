//! Developer commands over the USB-serial/JTAG port.
//!
//! Built only with the `debug-console` feature, so a production image carries no
//! reader task at all. Type a line and press Enter in the `espflash monitor`
//! window; the same port carries the defmt log, so the command echo is the only
//! sign it arrived.
//!
//! | Line        | Effect |
//! | ---         | --- |
//! | `ids`       | Log the next packet ID and the last message ID. |
//! | `reset-ids` | Set both counters back to zero and persist it. |

use embassy_time::{Duration, Timer};
use esp_hal::{Blocking, usb::usb_serial_jtag::UsbSerialJtag};

use crate::{packet::packet_data, storage::session::Flash};

/// Longest accepted command line; a longer line is dropped.
const LINE_LENGTH: usize = 32;
/// How often an idle console checks for input.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

const PRINT_IDS: &[u8] = b"ids";
const RESET_IDS: &[u8] = b"reset-ids";

/// Read command lines forever; the caller owns the port for the whole run.
pub async fn run(mut serial: UsbSerialJtag<'static, Blocking>, flash: &Flash) -> ! {
    let mut line = [0u8; LINE_LENGTH];
    let mut length = 0;

    loop {
        while let Ok(byte) = serial.read_byte() {
            match byte {
                b'\r' | b'\n' => {
                    if length > 0 {
                        handle(&line[..length], flash).await;
                        length = 0;
                    }
                }
                byte if length < LINE_LENGTH => {
                    line[length] = byte;
                    length += 1;
                }
                // Overlong line: discard it rather than run a truncated command.
                _ => length = 0,
            }
        }
        Timer::after(POLL_INTERVAL).await;
    }
}

/// Run one command line; unknown lines only log.
async fn handle(line: &[u8], flash: &Flash) {
    if line == PRINT_IDS {
        defmt::info!(
            "Next packet ID {:x}, last message ID {:x}",
            packet_data::current_packet_id(),
            packet_data::last_message_id()
        );
    } else if line == RESET_IDS {
        // Persist and rewind under the session lock so no producer reserves an
        // ID between the two steps.
        let saved = flash
            .with(async |session| {
                let saved = session
                    .storage
                    .save_packet_ids(0, 0, &mut session.buffer)
                    .await
                    .is_ok();
                packet_data::set_local_packet_ids(0, 0);
                saved
            })
            .await;
        if saved {
            defmt::info!("Packet IDs reset to 0");
        } else {
            defmt::warn!("Packet IDs reset in RAM but not in flash");
        }
    } else {
        defmt::warn!("Unknown console command; try `ids` or `reset-ids`");
    }
}
