# Architecture and Modules

## Runtime

This is a `no_std`, `no_main` ESP32-S3 firmware using `esp-hal` and Embassy. The application entry point is [`src/bin/main.rs`](../src/bin/main.rs).

Startup currently:

1. Initializes the ESP32-S3 at the maximum CPU clock and starts the Embassy runtime.
2. Initializes the ILI9341 on hardware SPI3 (GPIO12 SCK, GPIO11 MOSI) at 10 MHz. SPI2 is used by the LR1121. Settings and application data use the dedicated internal-flash partition; there is no SD-card backend or second-core storage worker.
3. Configures the radio SPI/control pins through `hardware/pinout.rs` and constructs `Lr1121`. The driver performs a hard reset and system startup: SPI CRC off, DCDC regulator, board RF switch, 3.0 V TCXO, 32.768 kHz crystal LFCLK, XOSC standby, full calibration, and IRQ/error clearing.
4. `Radio::new` applies the sub-GHz LoRa profile at 869.4 MHz, including image calibration, RSSI calibration, modem/packet parameters, configured LoRa sync word (default `0x12`), HP PA, 22 dBm TX power, 48 us ramp, RC-standby fallback, and DIO9 IRQ routing.
5. Starts GPS polling on UART1 (GPIO4/GPIO5) and publishes compact location/time snapshots to the UI through an Embassy watch.
6. Starts the radio worker alongside a diagnostic producer that queues an unencrypted broadcast text test packet every 30 seconds. This is not a user chat send path and does not save chat history.
7. Starts the display UI task. The logical chat flow supports direct text send/receive, encrypted ACK status, received GPS coordinate rows, and navigation; the physical button expander and flash-backed conversation history are not integrated.

Radio pin assignment (configured through `hardware/pinout.rs`):

| Signal | Pin |
| --- | --- |
| SPI SCK | GPIO40 |
| SPI MOSI | GPIO45 |
| SPI MISO | GPIO46 |
| LR1121 NSS | GPIO42 |
| LR1121 BUSY | GPIO41 |
| LR1121 RESET | GPIO39 |
| LR1121 IRQ | GPIO38 |

The LR1121 RF switch is configured inside the radio with `SetDioAsRfSwitch` using the board reference values (`RFSW0 | RFSW1`) and does not consume additional ESP32 GPIOs. Selecting a silicon band in software still does not prove that the board's antenna, matching network, front end, or regulatory domain supports it.

See the [HF-Kit peripheral pinout](pinout.md) for GPS, SPI TFT, and MCP23017 wiring. GPS and the TFT are initialized by firmware. The button expander is documented as a wiring plan and is not yet integrated.

## Source Map

| Module | Responsibility and current state |
| --- | --- |
| `hardware/pinout.rs` | Board wiring and constructors for radio and GPS pins. |
| `hardware/gps.rs` | Nonblocking UART1 polling and NMEA GGA/RMC parsing. Hold a `Gps` object, call `update()` to drain UART, read `coordinate`/`time` fields. No PPS support. |
| `crypto/mod.rs` | PBKDF2 pair-key derivation and software AES-256-GCM helpers. Nonces use the ESP-HAL RNG; paired direct text, coordinates, and ACK payloads use per-packet authenticated encryption. |
| `storage/storage_manager.rs` | Application-facing internal-flash API for settings, identity, contacts and message history. |
| `hardware/display.rs` | ILI9341 driver initialization and embedded-graphics display adapter. |
| `hardware/lr1121/lr1121.rs` | SPI transport, reset/BUSY/IRQ handling, system startup, RF-switch/TCXO/LFCLK setup, image and RSSI calibration, modem/PA/TX/RX commands, and LR-FHSS frame building. |
| `hardware/lr1121/opcode.rs` | Typed LR1121 system, radio, packet, and LR-FHSS opcodes used by the driver. |
| `network/packet_handler.rs` | Validates/deduplicates and forwards packets, reassembles authenticated direct text, delivers accepted text/coordinates to the chat UI, and queues ACKs for accepted direct messages. |
| `packet/packet_data.rs` | In-memory packet representation and validation, including the 16-fragment wire bound. |
| `packet/fragment.rs` | Band-aware text splitting and bounded reassembly of up to 512 application bytes per logical message. |
| `packet/packet_encoder.rs` | Serializes packet fields to the wire format. |
| `packet/packet_decoder.rs` | Parses and validates the fixed header and payload. |
| `packet/packet_type.rs` | Packet type byte assignments and conversion from a wire byte. |
| `radio/queue.rs` | Per-radio FIFO TX queue (16 encoded packets). No priority, emergency bypass, retries, persistence, or duplicate suppression. |
| `radio/control.rs` | Per-radio control channel for one pending local frequency-change request. It does not carry network packets. |
| `radio/radio.rs` | Applies per-band hardware configuration including the configured LoRa sync word, queues TX, services RX/TX IRQs, enforces a 30 s TX timeout, listens for up to 500 ms after every two queued transmissions, and retunes on worker requests. It routes IRQs to DIO9. |
| `radio/types/` | Configuration and value types for bands, PA paths, LoRa bandwidth/coding rate, the S-band LR-FHSS profile, and status values. |
| `hardware/flash.rs` | Internal-flash settings, contacts, and indexed message persistence over `sequential-storage`. |
| `storage/settings.rs` | Internal namespaced key serialization for settings and contacts. |
| `storage/contact.rs` | Fixed-width contact serialization. Stores a derived 32-byte key instead of a password; the key is not encrypted by this type. |

## Build and Check

Use the ESP toolchain environment and build the firmware target:

```sh
source /home/elias/export-esp.sh
cargo check --bin communicator
cargo build --bin communicator
```

The build is verified for `xtensa-esp32s3-none-elf`; the linker currently emits a generic RWX-load-segment warning. The configured Xtensa target does not provide Rust's `std`/test harness, and ESP-HAL rejects the host target, so `cargo test` cannot currently run for this project.

`cargo clippy --bin communicator` is not clean: `src/bin/main.rs` denies `clippy::large_stack_frames`, and the project currently exceeds that limit in several functions, including the radio worker and RX path. `cargo check` and `cargo build` are the working validation commands.

## Implemented vs Planned

- Packet framing, packet-type parsing, LoRa TX/RX on sub-GHz and 2.4 GHz, S-band LR-FHSS TX, and local retuning are implemented in code.
- LR1121 system startup, the board RF switch, TCXO/LFCLK setup, sub-GHz image calibration, RSSI calibration, PA selection, and DIO9 IRQ routing are implemented in code.
- LR-FHSS is transmit-only in this firmware. S-band RX is rejected/not started.
- `main.rs` starts RX and a 30-second sub-GHz test transmitter. A second board with the same frequency, modem, and sync word is required to observe received packets.
- Each radio uses its own FIFO queue (capacity 16). The worker listens for up to 500 ms after every two queued transmissions on bands that support RX. There is no public direct-radio send path, priority class, or emergency bypass. Automatic packet retries are not implemented; chat text fragmentation/reassembly is bounded to 16 frames and 512 application bytes.
- `FrequencyChange` is only a packet type. It has no announcement payload schema, handler, acceptance flow, or automatic retune behavior.
- `RadioControl::request_frequency_change` is a local worker command, separate from the `FrequencyChange` wire packet.
- Paired direct text, coordinates, and ACK payloads use AES-256-GCM with immutable packet headers as associated data; paired contacts reject plaintext fallback and invalid tags.
- Discovery/pairing, outgoing direct chat, bounded fragment reassembly, incoming chat delivery, received-coordinate navigation, and in-memory ACK indicators are implemented. ACK checks are shown only for encrypted direct messages and are not persisted.
- Flash storage and device identity are initialized by `main`; the diagnostic producer uses the ID for test packets. Settings, contacts, and message-history APIs exist, but chat messages and ACK state are not appended to flash.
- Antenna/front-end support, actual RF range, and regulatory approval remain board/application responsibilities. No hardware TX/RX result is recorded here.

## Storage Layout
- Settings, device identity, contacts and message records use `hardware/flash.rs` through `storage/storage_manager.rs`.
- The display uses hardware SPI3; the LR1121 uses SPI2.
- For message creation, append and paged-read examples, see [Storage and identity](storage-and-identity.md#saving-and-loading-messages).
- For ACK status, text fragmentation, coordinate wire format, and navigation limits, see [Chat, fragmentation, and GPS navigation](chat-fragmentation-and-navigation.md).
