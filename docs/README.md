# Communicator Firmware Docs

This directory contains all project Markdown documentation. Start with these implementation guides:

- [Architecture and modules](architecture.md): startup, source map, implemented behavior, and build command.
- [Packets and radio](packets-and-radio.md): packet layout, construction, TX queue, radio setup, bands, and retuning.
- [Storage and identity](storage-and-identity.md): initialization, settings, identity, contacts, and step-by-step message save/read examples.
- [Pinout and wiring](pinout.md): configured GPS/TFT wiring and the planned MCP23017 button expander.
- [Display plan and current UI](display-plan.md): implemented UI structure, screen behavior, and remaining work.
- [Protocol specification](protocol-specification.md): protocol design, with implementation status called out separately from intended behavior.

## Current Firmware Snapshot

- `cargo build --bin communicator` succeeds for `xtensa-esp32s3-none-elf`.
- Sub-GHz and 2.4 GHz LoRa custom-packet TX and RX are implemented.
- S-band LR-FHSS is transmit-only. S-band RX is not supported by the LR1121 modem path used here.
- `main.rs` runs a sub-GHz custom-packet test at 869.4 MHz: it queues an unencrypted broadcast `Text` packet every 30 seconds, starts RX, polls GPS on UART1, and updates the display UI. The test producer does not save messages to chat history.
- The firmware uses the private LoRa sync word (`0x12`), not the public LoRaWAN value (`0x34`).
- All TX goes through a 16-packet FIFO queue. After every two queued transmissions the worker listens for up to 500 ms before continuing. There is no priority path, retry logic, or fragmentation/reassembly.
- The packet handler validates and deduplicates received packets and forwards broadcasts or packets addressed to another device. Application delivery for locally addressed packets is still a TODO.
- Storage has append/read APIs for message history, but send/receive paths and the UI do not call them yet. The storage guide shows how to integrate them.
- No hardware TX/RX result is recorded in this repository. The build passes; antenna, RF-switch, range, and peer interoperability still require a two-board smoke test.

The [protocol specification](protocol-specification.md) describes intended protocol behavior. Where it describes encryption, fragmentation, forwarding, or regional profiles, read it as design guidance unless the implementation status in the guides says otherwise.

The LR1121 command reference used by this project is the supplied [LR1121 user manual](../62740776.LR1121_UM_V2.2.pdf). The firmware uses an in-tree SPI driver. It configures the board's system startup, RF switch, TCXO, image/RSSI calibration, modem, PA, and IRQ routing, but it is not a general-purpose implementation of every command or RF path in the manual.
