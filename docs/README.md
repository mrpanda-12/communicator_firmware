# Communicator firmware

Communicator is firmware for the Waveshare ESP32-S3 LR1121 HF board. It provides a small display-and-keypad interface for LoRa text and coordinate messages, stores conversations and paired contacts in the ESP32's internal flash, and receives GPS fixes over UART. This repository is the firmware source; it is not a desktop client or a hosted service.

The executable is `src/bin/main.rs`. It is `no_std`/`no_main`, uses ESP-HAL and Embassy, and targets `xtensa-esp32s3-none-elf`. Board wiring and runtime initialization are explicit in the source; the project does not discover peripherals automatically.

## What the firmware does

- Runs one LR1121 radio worker. The default profile is sub-GHz LoRa at 869.4 MHz, SF9, 125 kHz, coding rate 4/5, private LoRa sync word `0x12`, and configured transmit power 22 dBm.
- Sends and receives public broadcast text and direct text. Direct traffic is encrypted only when a contact has a saved pairing key and the user has not chosen the unencrypted introduction path.
- Fragments text according to the active radio's packet-size limit, reassembles bounded incoming messages, and stores complete accepted messages in flash.
- Pairs contacts from a locally entered shared secret. Paired direct packets use AES-256-GCM; the secret is not transmitted.
- Sends coordinates from the latest GPS fix, stores received coordinates as chat rows, and can show a simple distance/bearing screen for a selected row.
- Drives a 320×240 ILI9341 display and an MCP23017 5×5 keypad. The matrix scanner waits on the expander interrupt and debounces presses.
- Shows UTC from the GPS receiver and can display a fixed, persistent UTC offset next to it.
- Uses the `storage` partition in `partitions.csv` for identity, contacts, settings, and chat records. It does not use the TFT's microSD socket.
- Keeps each unacknowledged direct message in a two-slot RAM outbox, retransmits it after 20 s up to three times, and retries any fragment the TX queue refused.
- Enforces a 10 % transmit duty cycle per RF band with a token bucket; transmissions wait for budget instead of exceeding it.
- Persists the packet-ID counter in flash and exposes it, with the remaining percentage, under Settings → Device → Packet IDs.
- Caps each conversation at 128 stored records and drops the oldest one, and restarts the firmware when a service task stops.

The default firmware also queues the diagnostic broadcast text `Hello from the communicator` every 30 seconds. This is in `main.rs`; it is not a chat send and is useful only for development/bring-up.

## Documentation map

| Guide | Read it for |
| --- | --- |
| [Architecture and runtime](architecture.md) | Startup sequence, task ownership, and how events cross subsystem boundaries. |
| [User interface](display-plan.md) | Screen behavior, keypad mapping, settings, and scrolling chat history. |
| [Messaging workflow](messaging.md) | One message from user input or RX through queue/flash/UI, including failure paths. |
| [Packet handler](packet-handler.md) | RX validation, deduplication, authentication, reassembly, ACKs, and relay decisions. |
| [Protocol reference](protocol-specification.md) | Wire header, packet types, cryptographic framing, pairing stages, and payload encodings. |
| [Radio and packet path](packets-and-radio.md) | Radio configuration, queue ownership, worker behavior, and band limits. |
| [Storage and identity](storage-and-identity.md) | Partition bounds, map keys, record lifecycle, identity reset, and flash-write behavior. |
| [Chat history and navigation](chat-fragmentation-and-navigation.md) | History paging, message indicators, fragmentation limits, and coordinate navigation. |
| [Board wiring](pinout.md) | Pin assignments, keypad matrix, bring-up, and electrical cautions. |

## Build and run

The repository config selects `xtensa-esp32s3-none-elf` in `.cargo/config.toml` and uses unstable `build-std` support. Install the Espressif Rust toolchain with `espup`, source its environment export, and install `espflash` before building. The exact export-script path depends on the local installation.

```sh
source /path/to/export-esp.sh
cargo fmt --check
cargo check
cargo build --release
```

Compiling requires the Xtensa linker, which the espup environment provides; `build.rs` stops with setup instructions when it is missing. The Cargo runner is configured to flash and monitor using `partitions.csv`; flashing a board is a separate hardware operation.

No host test suite currently exercises the full firmware. A small set of tests is embedded in `ui/render/top_bar.rs`, but the configured target is Xtensa. Do not treat formatting or a successful compile as evidence of RF, display, GPS, flash-endurance, or keypad operation. The keypad and radio need a board for physical verification.

## First boot and radio pairing

1. Confirm that the flashed partition table contains the `storage` data region at `0x210000` with size `0x1f0000`. The firmware opens the absolute range `0x210000..0x400000`; writes can erase pages inside it.
2. On first boot, the firmware creates a 16-byte device ID using the ESP hardware TRNG and saves it. If flash or the entropy source fails, identity-dependent traffic is unavailable.
3. Put two compatible boards on the same supported profile. A receiver must use the same modulation parameters and sync word as the sender. The current default config is sub-GHz LoRa; a local settings retune does not inform the other board.
4. Let each board hear a packet or use the contacts screen's scan action to send a `DeviceAnnounce`. Discovery is passive: this firmware does not reply with its identity automatically.
5. Pair on both ends by entering the same strong secret. The keypad enters two hex digits per ASCII character. A minimum length is enforced in the UI, but the code does not make a low-entropy secret safe from offline guessing.
6. Send a direct message. It is encrypted when the pair key is saved. The receiver stores it before producing an authenticated ACK. The sender's encrypted direct row changes from “SENDING..” to the acknowledged state only after that ACK is received and recorded.

Broadcast traffic and explicitly selected unpaired introductions are plaintext. Do not use those paths for private content.

## Important limits

- The wire `packet_id` and `message_id` fields are 32-bit values. Local packet IDs come from one flash-persisted counter shared by the UI, radio receive responses, pairing packets, and diagnostic sender; it wraps only after 4,294,967,296 reservations. The stored value stays at least 1024 IDs ahead of the running counter, so a power loss can skip IDs but never replay one. The settings screen's PACKET IDS page shows the last packet and message ID, the next ID, and the remaining percentage.
- Protocol version 2 is not backward compatible: both boards must run firmware that uses the 47-byte header. A board running version 1 rejects version 2 frames, and the reverse is also true. Chat records written by version 1 firmware use the narrower ID layout and cannot be read by this firmware; delete the old conversations from the settings screens after upgrading.
- The TX queue holds 32 encoded packets. The outbox keeps two messages for delivery; a third message is refused on screen and stays in the composer. The outbox is not persisted, so a reboot cancels pending retries.
- A direct message is retransmitted after 20 s while the peer has not acknowledged it, up to three transmissions in total. A retry reuses the original encoded fragments, so the receiver's duplicate cache suppresses a second stored row. Broadcast and plaintext sends are not retried.
- RX duplicate state and incomplete-fragment assemblies are RAM-only. The receiver retains at most 128 seen IDs, 128 accepted ACK records, and four incomplete messages. Before appending a received record, the newest eight records of that conversation are checked, so a retransmit after a reboot does not create a duplicate row.
- Each conversation keeps at most 128 records; the oldest record is deleted when the next one is appended. Chat screens page through stored history, four records at a time.
- The duty-cycle limiter accounts per RF band over a one-hour window at 10 %. A fresh boot starts with 6 s of credit and accrues 0.1 s per second, capped at 360 s.
- The firmware's code supports LoRa receive on sub-GHz and 2.4 GHz profiles. S-band LR-FHSS is transmit-only in this radio implementation. The current board antenna/front-end suitability must be checked before changing bands.
- Identity reset replaces the device ID but retains local contact and history records. Other devices must pair again to the new ID; the local contact book is not automatically migrated.

## Source and hardware references

The pin map is maintained in [`src/bin/hardware/pinout.rs`](../src/bin/hardware/pinout.rs), and low-level LR1121 commands are in [`src/bin/hardware/lr1121/`](../src/bin/hardware/lr1121/). The supplied LR1121 manual is [`62740776.LR1121_UM_V2.2.pdf`](../62740776.LR1121_UM_V2.2.pdf). See [Board wiring](pinout.md) before connecting external modules. Verify the board revision's schematic and regulatory requirements; the radio's supported silicon range is not a certification that a specific antenna or region supports a transmission.