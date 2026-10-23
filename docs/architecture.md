# Architecture and Modules

## Runtime

This is a `no_std`, `no_main` ESP32-S3 firmware using `esp-hal` and Embassy. The application entry point is [`src/bin/main.rs`](../src/bin/main.rs).

Startup currently:

1. Initializes the ESP32-S3 at the maximum CPU clock and starts the Embassy runtime.
2. Reserves GPIO27-GPIO32, then creates SPI2 for the LR1121 at 1 MHz, SPI mode 0.
3. Configures the radio SPI/control pins through `hardware/pinout.rs` and constructs `Lr1121`. The driver performs a hard reset and system startup: SPI CRC off, DCDC regulator, board RF switch, 3.0 V TCXO, 32.768 kHz crystal LFCLK, XOSC standby, full calibration, and IRQ/error clearing.
4. `Radio::new` applies the sub-GHz LoRa profile at 869.4 MHz, including image calibration, RSSI calibration, modem/packet parameters, private sync word `0x12`, HP PA, 22 dBm TX power, 48 us ramp, RC-standby fallback, and DIO9 IRQ routing.
5. Leaves UART1 on GPIO4/GPIO5 available for GPS via `pinout::gps_pins()`; GPS is not started by `main.rs`.
6. Starts the radio worker alongside a test producer that queues a broadcast `Text` packet every 30 seconds.

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

See the [HF-Kit peripheral pinout](pinout.md) for GPS, SPI TFT, and MCP23017 wiring. GPS is configured in firmware; the display and button-expander assignments are still proposed.

## Source Map

| Module | Responsibility and current state |
| --- | --- |
| `hardware/pinout.rs` | Board wiring and constructors for the configured radio control pins, GPS UART pins, and future display/button-expander GPIO constants. |
| `hardware/gps.rs` | Nonblocking UART1 polling and NMEA GGA/RMC parsing. Hold a `Gps` object, call `update()` to drain UART, read `coordinate`/`time` fields. No PPS support. |
| `crypto/mod.rs` | Software AES-256-GCM `derive_key`, `encrypt`, and `decrypt` free functions. Nonces use the ESP-HAL RNG. Packet encoding does not call these yet. |
| `device/identity.rs` | Loads or explicitly regenerates a 16-byte device ID with ESP-HAL `Trng`, persisting it as the `device_id` setting. Not called by `main` yet. |
| `hardware/display.rs` | Empty; no display driver is implemented. |
| `hardware/lr1121/lr1121.rs` | SPI transport, reset/BUSY/IRQ handling, system startup, RF-switch/TCXO/LFCLK setup, image and RSSI calibration, modem/PA/TX/RX commands, and LR-FHSS frame building. |
| `hardware/lr1121/opcode.rs` | Typed LR1121 system, radio, packet, and LR-FHSS opcodes used by the driver. |
| `network/packet_handler.rs` | Decodes received packets and prints their metadata and unencrypted payload. Malformed input is reported without panicking. It does not route or forward packets. |
| `network/packet_router.rs` | Empty; forwarding and duplicate detection are not implemented. |
| `packet/packet_data.rs` | In-memory packet representation and basic validation. |
| `packet/packet_encoder.rs` | Serializes packet fields to the wire format. |
| `packet/packet_decoder.rs` | Parses and validates the fixed header and payload. |
| `packet/packet_type.rs` | Packet type byte assignments and conversion from a wire byte. |
| `radio/queue.rs` | Static FIFO TX queue (16 encoded packets) and one pending local frequency-change request. No priority, emergency bypass, retries, persistence, or duplicate suppression. |
| `radio/radio.rs` | Applies per-band hardware configuration, queues TX, services RX/TX IRQs, enforces a 30 s TX timeout, listens for up to 500 ms after every two queued transmissions, and retunes on worker requests. It sets the private LoRa sync word and routes IRQs to DIO9. |
| `radio/types/` | Configuration and value types for bands, PA paths, LoRa bandwidth/coding rate, the S-band LR-FHSS profile, and status values. |
| `storage/flash.rs` | Typed settings and contact persistence over `sequential-storage` and ESP flash. |
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
- All TX uses a single FIFO queue (capacity 16). The worker listens for up to 500 ms after every two queued transmissions on bands that support RX. There is no public direct-radio send path, priority class, or emergency bypass. Retries, fragmentation/reassembly, and duplicate suppression are not implemented.
- `FrequencyChange` is only a packet type. It has no announcement payload schema, handler, acceptance flow, or automatic retune behavior.
- A local `request_frequency_change` is separate from the `FrequencyChange` wire packet.
- AES-256-GCM primitives exist in `crypto/mod.rs`. An `is_encrypted` flag still does not encrypt or authenticate data by itself because the packet path does not call them.
- Fragmentation, reassembly, forwarding, duplicate suppression, contact-based authorization, and display support are not implemented.
- Flash storage and device identity APIs exist but are not initialized or called by `main`.
- Antenna/front-end support, actual RF range, and regulatory approval remain board/application responsibilities. No hardware TX/RX result is recorded here.

## Message Storage
`storage/message.rs` defines `StoredMessage`, `MessageKind` (Text/Coords), and `MAX_MESSAGES_PER_CONVO` (50). `FlashStore` persists messages per device ID (DMs) and broadcast separately. This enables chat history with capped history and fast loading per conversation.
