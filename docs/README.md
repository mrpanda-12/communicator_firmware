# Communicator Firmware Docs

This directory documents the current firmware implementation. Start with:

- [Architecture and modules](architecture.md): startup, source map, implemented behavior, and build command.
- [Packets and radio](packets-and-radio.md): packet layout, construction, TX queue, radio setup, bands, and retuning.
- [Storage and identity](storage-and-identity.md): flash partition requirements, typed settings, contacts, and device IDs.
- [Pinout plan](pinout.md): proposed GPS, ILI9341 TFT, and MCP23017 wiring while preserving LR1121 and UART0 pins; includes GPS driver usage.

## Current Radio Status

- `cargo build --bin communicator` succeeds for `xtensa-esp32s3-none-elf`.
- Sub-GHz and 2.4 GHz LoRa custom-packet TX and RX are implemented.
- S-band LR-FHSS is transmit-only. S-band RX is not supported by the LR1121 modem path used here.
- `main.rs` runs a sub-GHz custom-packet test at 869.4 MHz: it queues a broadcast `Text` packet every 30 seconds and starts RX.
- The firmware uses the private LoRa sync word (`0x12`), not the public LoRaWAN value (`0x34`).
- The packet handler decodes received packets and prints them over the serial console. It does not route, forward, or acknowledge them.
- No hardware TX/RX result is recorded in this repository. The build passes; antenna, RF-switch, range, and peer interoperability still require a two-board smoke test.

The top-level [protocol specification](../COMMUNICATOR_PROTOCOL_SPECIFICATION.md) describes intended protocol behavior. Where it describes encryption, fragmentation, forwarding, or regional profiles, read it as design guidance unless the implementation status in the guides says otherwise.

The LR1121 command reference used by this project is the supplied [LR1121 user manual](../62740776.LR1121_UM_V2.2.pdf). The firmware uses an in-tree SPI driver. It configures the board's system startup, RF switch, TCXO, image/RSSI calibration, modem, PA, and IRQ routing, but it is not a general-purpose implementation of every command or RF path in the manual.