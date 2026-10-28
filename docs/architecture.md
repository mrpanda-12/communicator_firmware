# Architecture

## Runtime

Firmware for the ESP32-S3 running Embassy (`no_std`, `no_main` via `esp-hal`).
Entry point: [`src/bin/main.rs`](../src/bin/main.rs).

Startup order:

1. Initialize the MCU at maximum CPU clock and start the Embassy runtime.
2. Initialize the internal-flash `storage` partition, the TRNG, and the device
   identity; load persisted settings (backlight, UTC offset) and contacts.
3. Initialize the ILI9341 display on SPI3 and drive the backlight with LEDC PWM.
4. Configure the LR1121 on SPI2: sub-GHz LoRa at 869.4 MHz, HP PA at 22 dBm,
   sync word `0x12`, IRQs on DIO9.
5. Start the radio worker, the GPS poller (UART1), the keypad worker (I2C0),
   and the diagnostic test-packet producer.
6. Run the UI task. It renders, polls flash through the session handle, and
   selects on keypad, GPS, radio-config, discovery, pairing, and popup events.

Everything after step 6 runs concurrently; ownership decides who may touch
which peripheral:

| Task | Owns | Publishes |
| --- | --- | --- |
| Radio worker | LR1121, its TX queue | packets via the receive callback |
| GPS task | UART1/NMEA | `GpsSnapshot` on a watch |
| Keypad worker | MCP23017 bus | keys on the `KEYPAD_EVENTS` channel |
| Packet handler | none (runs in the radio worker future) | flash writes, popup/revision notifications, pair contacts |
| UI task | display, `UiState`, `ContactBook` | outgoing packets, manage commands, settings writes |

## Data flow

```
radio RX ─▶ packet_handler ─▶ StorageManager (flash records)
                       │
                       ├▶ notify::receive   → popup queue + revision
                       └▶ discovery/pairing → contact adoption

keypad ─▶ KeypadQueue ─▶ UiState::handle_keypad ─▶ pending_* commands
                                                        │
              UI run loop ◀──────────────────────────── ┘
                │  (send, manage, delete-all, timezone save)
                ├▶ RadioQueue (fragments)      ├▶ StorageManager
                └▶ render (diff-based redraw)
```

The receive path never touches the display. It writes accepted messages to
flash and signals the UI with a popup and a history revision; the UI reloads
only what changed. There is no second copy of chat history in RAM.

## Source map

| Module | Responsibility |
| --- | --- |
| `hardware/pinout.rs` | Board pin constructors. |
| `hardware/gps.rs` | Nonblocking UART polling, NMEA GGA/RMC parsing; time with optional date. |
| `hardware/display.rs` | ILI9341 init and embedded-graphics adapter. |
| `hardware/keypad.rs` | MCP23017 matrix scan, debounce, IRQ-driven worker, key queue. |
| `hardware/flash.rs` | Settings, contacts, and message records over `sequential-storage`. |
| `hardware/lr1121/` | In-tree LR1121 SPI driver: startup, calibration, modem, PA, LR-FHSS frames. |
| `crypto/mod.rs` | PBKDF2 pair-key derivation and AES-256-GCM packet protection. |
| `packet/` | Wire format: `PacketData`, serializer/deserializer, types, fragmenter/reassembler. |
| `radio/queue.rs` | Per-radio FIFO TX queue (16 packets). |
| `radio/control.rs` | Pending local retune request channel. |
| `radio/radio.rs` | Band configuration, TX/RX IRQ loop, RX windows, retuning. |
| `network/packet_handler.rs` | Receive pipeline: validate, dedup, decrypt, deliver to flash, ACK, relay, pairing. See [packet handler](packet-handler.md). |
| `network/discovery.rs` | Bounded nearby-device list on a watch channel. |
| `network/pairing.rs` | Shared-secret handshake state machine and packet builders. |
| `notify/mod.rs` | Popup queue and history-revision watch; the receive path's only UI channel. |
| `storage/` | `StorageManager` facade, contact/message records, namespaced flash keys, shared `Flash` session. |
| `ui/state.rs` | Screen state machine, key handling, pending commands, chat rows. |
| `ui/contact.rs` | In-RAM contact book with chat-list previews. |
| `ui/render/` | Per-screen renderers plus diff-based redraw and the top bar. |
| `ui/navigation.rs` | Distance/bearing math for the navigation screen. |

Storage detail: [storage and identity](storage-and-identity.md). UI detail:
[user interface](display-plan.md).

## Build and check

```sh
source /home/elias/export-esp.sh
cargo build --release                      # xtensa-esp32s3-none-elf
cargo build --release --no-default-features
```

Both configurations compile clean; the linker emits a generic RWX-segment
warning. Clippy's `large_stack_frames` lint (denied at 1 KiB) still fires for
a few functions in the radio and receive paths; `cargo check`/`build` are the
working gates.

`cargo test` does not run on the host because of the Xtensa target; unit tests
kept in the source tree (date conversions in `ui/render/top_bar.rs`) would
need a host-testable split or an ESP test harness to execute.

## Known limits

- The radio worker services one packet at a time; RX pauses during TX.
- Duplicate suppression and fragment reassembly state are RAM-only and reset
  on reboot; a retransmitted single-fragment message can therefore be shown
  twice after a restart.
- Chat history grows without a retention policy; the storage partition is
  finite and append errors surface to the send path.
- The keypad has not been validated on hardware; a missing expander only
  removes keypad input.
- No RF range or interoperability test is recorded in this repository.
