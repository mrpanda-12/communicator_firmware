# Runtime architecture

The firmware executable is `src/bin/main.rs`. It initializes one ESP32-S3 board and then runs the display/UI loop alongside long-lived radio, GPS, and keypad futures. It uses Embassy async execution on an Xtensa target; there is no operating-system process model or desktop application layer.

## Boot sequence

`main` performs setup in this order:

1. Initialize ESP-HAL at the maximum configured CPU clock, install the project heap, and start the Embassy runtime using TIMG0.
2. Create the internal-flash map over `0x210000..0x400000`. Those bounds match the `storage` row in `partitions.csv`; the storage layer can erase pages inside that range.
3. Start the TRNG entropy source. Read or create the 16-byte identity, choose a random starting value for the per-boot local packet-ID allocator, and load brightness, UTC offset, and contacts from flash.
4. Move storage plus its reusable 640-byte scratch buffer into the shared `Flash` mutex. The receive path and UI take short exclusive turns through this handle.
5. Initialize the ILI9341 on SPI3, its GPIO7 PWM backlight, the LR1121 on SPI2, the GPS on UART1, and the MCP23017 keypad expander on I2C0. A failure here, or in one of the shared watches, is not recoverable, but the board reports it: `hardware/errorscreen.rs` draws a startup error screen with the reason, counts down ten seconds, and calls `software_reset`. Failures before the panel is up (flash range, TRNG, display bus or panel) cannot be drawn; `fatal` logs `Boot stopped: <reason>` and reboots after the same ten seconds. The firmware never continues with a missing peripheral.
6. Publish the device identity through `DEVICE_ID`, construct the radio worker and its packet handler, and enter the event-driven UI loop. Nothing is queued at boot: the only unsolicited transmission is the `DeviceAnnounce` broadcast from the contacts screen's ADD DEVICE scan.

The partition geometry and board wiring are described in [storage and identity](storage-and-identity.md) and [board wiring](pinout.md).

## Runtime ownership

| Runtime component | Owns or operates | Communication with other components |
| --- | --- | --- |
| Radio worker | LR1121 SPI peripheral, TX/RX state, IRQ handling | Takes encoded packets from `RADIO_QUEUE`; passes RX frames to `PacketHandler`; observes `RADIO_CONTROL`. |
| `PacketHandler` | Receive caches and fragment reassembler, within the radio worker future | Reads `DEVICE_ID`; queues ACKs/relays; uses `Flash`; publishes discovery and chat notifications; advances pairing. |
| UI loop | Display, `UiState`, in-memory contact list | Receives keypad, GPS, radio-config, pairing, discovery, popup, and history-change events; queues outbound packets; persists user actions through `Flash`. |
| GPS future | UART1 and NMEA parser | Publishes changed `GpsSnapshot` values through a watch. |
| Keypad worker | MCP23017 I2C bus and GPIO16 interrupt | Sends debounced `KeypadKey` events into `KEYPAD_EVENTS`. |
| Flash session | Flash map and 640-byte work buffer under one mutex | `Flash::with` serializes access from the UI and receive handler. |

The radio and keypad workers own their hardware instead of sharing mutable peripheral access with the UI. `RADIO_QUEUE`, `RADIO_CONTROL`, `DEVICE_ID`, and GPS snapshots are static shared channels/watches. The `Flash` mutex ensures the UI cannot reuse the flash map or work buffer while packet handling is storing a message.

## Message paths

### Sending

```text
5x5 keypad press
    → keypad event channel
    → UiState composer and send menu
    → UI checks device identity and direct-contact key
    → fragment count and packet IDs reserved
    → band-sized PacketData fragments built
    → AES-256-GCM per fragment for paired direct traffic
    → outbox (two RAM slots, all fragments of the message)
    → RADIO_QUEUE (encoded FIFO frames)
    → radio worker, duty-cycle wait, LR1121 TX
    → one logical sent-message record appended to flash
    → UI history revision reload
```

The local packet-ID allocator is a shared 32-bit sequence persisted in flash. It reserves a contiguous range for a fragmented message; pairing, ACK, announcement, and chat producers all draw from that same allocator. Each reservation persists the advanced counter before the IDs are handed out, so at boot the counter resumes at the first unused ID and a power loss can skip an interrupted reservation but never replay a sent one. The wire fields are 32 bits wide, so the counter wraps only after 4,294,967,296 reservations. The Packet IDs settings page shows the counters and the remaining percentage.

The outbox owns a message until the peer acknowledges it: an outbox task retries fragments that the TX queue refused and retransmits the whole message after 20 s, up to three transmissions. An authenticated ACK cancels the retry. There is no persistent TX queue, so a reboot loses pending retries. Details and failure behavior are in [messaging workflow](messaging.md).

### Receiving

```text
LR1121 RX IRQ
    → radio worker reads one encoded frame and restarts RX
    → PacketHandler parses and validates the packet
    → self/duplicate/destination checks
    → key lookup and authenticated decryption when direct and paired
    → fragment assembly or coordinate validation
    → one flash record for the complete logical message
    → popup + history revision
    → UI reloads visible rows and list preview from flash
    → authenticated ACK queued after successful persistence (direct paired only)
```

Broadcasts and packets for other devices may be forwarded after hop-limit decrement. The RX worker does not draw pixels or hold a second chat-history cache. Read [packet handler](packet-handler.md) for the ordering and packet-type matrix.

## UI and event flow

`UiState` is the screen state machine and command producer. A key mutates screen state or sets one pending command (send, pairing, announcement, identity reset, frequency request, contact action, deletion, or history-page request). The UI loop consumes those commands and performs radio/storage operations outside the renderer. `ui/render/` functions draw the state; `draw_if_changed` repaints the affected region or page rather than running storage or networking logic during rendering.

Received messages use `notify::message_received`: one bounded popup is queued and the history revision is incremented. A revision wake causes an open chat to reload its newest page; list previews are read from flash on boot or when a message notification arrives. Pressing Up/Down while the chat composer is empty browses records in pages of four; the selected row can open coordinate navigation. See [user interface](display-plan.md) and [chat history](chat-fragmentation-and-navigation.md).

## Module responsibilities

| Path | Responsibility |
| --- | --- |
| `hardware/pinout.rs` | GPIO-to-peripheral mapping. |
| `hardware/display.rs` | ILI9341 initialization. |
| `hardware/errorscreen.rs` | Startup error screen: reason, ten-second countdown, reboot. |
| `hardware/keypad.rs` | MCP23017 setup, 5×5 scan, debounce, interrupt worker, event queue. |
| `hardware/gps.rs` | UART receive and NMEA parsing. |
| `hardware/flash.rs` | Typed map access, contact records, message pages and deletion. |
| `hardware/lr1121/` | LR1121 command protocol and board-specific radio initialization. |
| `crypto/mod.rs` | Pair-key derivation, AES-256-GCM, and packet fingerprints. |
| `packet/` | Packet model/codec, type values, fragmentation, reassembly, local packet-ID allocator. |
| `radio/` | Configuration, TX FIFO, retune channel, LR1121 worker. |
| `network/` | Discovery list, pairing coordinator, receive and relay path. |
| `storage/` | Message/conversation model, contact model, key namespaces, shared flash session. |
| `notify/` | Bounded popup channel and chat-history revision signal. |
| `ui/state.rs` | Screens, input behavior, commands, history-page selection, display-ready rows. |
| `ui/render/` | TFT drawing by page and widget. |

## Design boundaries and current limitations

- `PacketData` is the application framing above the LR1121 PHY; modulation, sync word, CRC, and IRQ are radio configuration, not packet fields.
- Radio IDs and receiver duplicate/reassembly caches are finite and volatile. The wire ID widths cannot promise permanent uniqueness.
- Storage is serialised because the map and scratch buffer are shared. Each sent/received chat message is persisted once per logical message; ACK status updates the sent record separately.
- The outbox retransmits a whole direct message; there is no per-fragment resend. If a fragment is lost, the receiver does not deliver the incomplete message, so the sender's retry sends the complete set again.
- `DeviceAnnounceResponse` is user-facing only as the ASK TO ADD request: plaintext, empty payload, nothing stored on receipt. `Heading` and `FrequencyChange` exist in the packet enum but have no sender or handler, and local frequency selection does not coordinate peers.
- Discovery IDs and unpaired plaintext introductions are unauthenticated. Contact pair keys are stored as serialized key bytes, not encrypted at rest.
- No automated test can be built for this target: `cargo test` stops at `error[E0463]: can't find crate for 'test'` because the Xtensa `no_std` toolchain ships no `test` crate, so the embedded `#[cfg(test)]` module never executes. Nothing in this tree is proof of RF range, keypad soldering, storage endurance, or a physical board run.

Use [protocol reference](protocol-specification.md) for wire and cryptographic details, [radio guide](packets-and-radio.md) for worker operation, and [storage guide](storage-and-identity.md) for persistence.