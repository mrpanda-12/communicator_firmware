# Communicator Firmware

A `no_std` ESP32-S3 + LoRa messenger. Two boards exchange broadcast and direct
messages over the LR1121 radio, store chat history on the internal flash
partition, and pair contacts with a password-derived AES-256 key. The board
carries an ILI9341 TFT, a 5x5 MCP23017 keypad, and an NMEA GPS receiver.

Documentation map:

| Document | Contents |
| --- | --- |
| [Architecture](architecture.md) | Runtime structure, startup order, and the module map. |
| [Protocol specification](protocol-specification.md) | Wire format, IDs, key exchange, fragmentation, payload formats. |
| [Packets and radio](packets-and-radio.md) | Band capabilities, sync words, transmit queue, retuning. |
| [Packet handler](packet-handler.md) | Receive pipeline: validation, dedup, delivery, ACK, relaying, pairing. |
| [Message delivery](messaging.md) | How an accepted message becomes a flash record, a popup, and a chat row. |
| [Storage and identity](storage-and-identity.md) | Flash partition, settings, device identity, contacts, message records. |
| [User interface](display-plan.md) | Screens, menus, keypad, top bar, and UI conventions. |
| [Chat and navigation](chat-fragmentation-and-navigation.md) | ACK semantics, fragment limits, coordinate messages, navigation. |
| [Pinout](pinout.md) | Display, GPS, and keypad wiring. |

## Feature status

- LoRa TX/RX on sub-GHz (default 869.4 MHz) and 2.4 GHz; LR-FHSS is transmit-only.
- Broadcast and direct chat: text and GPS coordinates, sent and received.
- Encrypted direct traffic between paired contacts (AES-256-GCM, pair key from a
  shared secret). Plaintext introductions are possible and clearly warned about.
- Chat history persists to flash; the UI reads records back per conversation.
- Received direct messages pop up over any screen and appear in the chats list;
  peers that were never paired show in amber until paired.
- ACK for accepted encrypted direct messages turns the sent row from grey
  "SENDING.." to green "OK"; the state persists in the message record.
- Contact management: block or delete a contact (or the whole chat history)
  from the chats or contacts lists.
- GPS: fix status, coordinates, UTC clock (date included), and an optional
  UTC offset for a local clock in the top bar.

Not implemented: automatic retries, LR-FHSS RX, a frequency-change protocol,
SD-card storage, and hardware-verified keypad or RF results.

## Building

The Xtensa toolchain must be active:

```sh
source /home/elias/export-esp.sh
cargo build --release
```

`cargo build --release` targets `xtensa-esp32s3-none-elf` (configured in
`.cargo/config.toml`). The linker reports one known warning (an RWX load
segment); builds are otherwise clean. `cargo test` cannot run on the host for
this target, so the in-file unit tests (for example the date arithmetic in
`ui/render/top_bar.rs`) are not executed by the current CI path.

A `fake-navigator` feature (on by default) injects keypad traffic so the UI can
be driven without the expander; flash with `--no-default-features` for normal
operation on hardware.

## Flashing and first run

```sh
cargo espflash flash --release --target xtensa-esp32s3-none-elf
```

First boot creates a random 16-byte device identity on the `storage` flash
partition (see `partitions.csv`) and leaves the UTC offset unset. Nothing in
this repository documents a two-board RF range test; run one before relying on
the link.

## Hardware reference

The firmware drives a Waveshare ESP32-S3 + LR1121 HF LoRa kit. The LR1121 is
configured through an in-tree SPI driver (`hardware/lr1121/`) aimed at this
board's reference startup sequence; it is not a general-purpose
implementation of the whole command set. The vendor command reference is
[LR1121 user manual](../62740776.LR1121_UM_V2.2.pdf).
