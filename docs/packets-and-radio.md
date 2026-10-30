# Radio and transmit path

The LR1121 driver in `src/bin/hardware/lr1121/` and the worker in `src/bin/radio/radio.rs` together move protocol packets on and off the air. The protocol framing is separate; see [protocol reference](protocol-specification.md) for the 47-byte header and payload rules.

## Current configuration

`main.rs` builds one fixed config:

```rust
RadioConfig {
    frequency_band: FrequencyBand::SubGhz,
    bandwidth: LoraBandwidth::Khz125,
    pa_mode: PaMode::HighPower,
    tx_power_dbm: 22,
    ..RadioConfig::new(869_400_000)
}
```

`RadioConfig::new` fills in spreading factor 9, coding rate 4/5, preamble 8 symbols, CRC enabled, explicit header, non-inverted IQ, LoRa sync word `0x12`, and automatic low-data-rate-optimization. The default frequency here is 869.4 MHz, while `RadioConfig::default()` uses 869.5 MHz with the low-power PA; check which value a code path actually uses before assuming a link will match.

A working link needs matching frequency, bandwidth, spreading factor, coding rate, preamble, header mode, CRC, and sync word on both ends. Sync word is a modem filter, not a secret: `0x12` is a private-network value and `0x34` is the LoRaWAN public value, so LoRaWAN devices do not decode these packets. It is configured in the radio, never written into the 47-byte protocol packet.

`RadioConfig::validate` rejects a spreading factor outside 5–12, a bandwidth not encodable for the band, a zero preamble, a PA mode that does not match the band, transmit power outside −9…selected PA maximum, and a frequency outside the band's range. The worker enforces a 10 % transmit duty cycle per band; transmit power and antenna limits remain the operator's responsibility.

## Transmit and receive capabilities

| Band | Range | Modem | RX | Maximum encoded packet |
| --- | --- | --- | --- | ---: |
| `SubGhz` | 150–960 MHz | LoRa | Yes | 255 bytes |
| `SBand` | 1.9–2.1 GHz | LR-FHSS | No; TX only | 133 bytes |
| `Ism2_4Ghz` | 2.4–2.5 GHz | LoRa | Yes | 255 bytes |

LoRa bandwidths supported by the driver: sub-GHz 62.5/125/250/500 kHz; 2.4 GHz also 203/406/812 kHz. PA modes cap transmit power at 14 dBm (low power), 22 dBm (high power sub-GHz path), and 13 dBm (high-frequency path used for S-band and 2.4 GHz). This board's sub-GHz output is wired through the HP PA configuration for both sub-GHz modes.

S-band uses the `SBandDefault` LR-FHSS profile: GMSK modulation, 5/6 coding rate, 136.72 kHz bandwidth, hopping enabled, two header blocks, and sync word `0x2C0F7995`. The LR1121 LR-FHSS implementation is transmit-only, and the worker does not start RX while configured for S-band. S-band requires an antenna/front end that actually supports 2 GHz.

These are radio capabilities, not evidence that this board's RF path, antenna, or enclosure works in any band. No RF range or interoperability test is recorded here.

## Duty cycle

`Radio::run_worker` charges every frame against a token bucket for its band before starting TX. Each bucket accrues airtime at 10 % of real time, caps at 360 s, and starts a boot with 6 s of credit. A frame whose airtime exceeds the available credit makes the worker wait until the credit covers it, so a busy queue is paced rather than silently dropped.

The charge comes from `RadioConfig::time_on_air_us`: the Semtech LoRa time-on-air formula for sub-GHz and 2.4 GHz (spreading factor, bandwidth, coding rate, preamble, header mode, CRC, LDRO), and a bit-rate estimate derived from the LR-FHSS bandwidth for S-band. The S-band figure is conservative; the limit is an accounting device, not a regulatory certification of the modem profile.

While the worker waits for budget it does not service the RX interrupt, so a long wait can miss an inbound frame. The budget is charged before the modem starts TX, and a frame refused by the band limit is dropped rather than stopping the worker.

## Ownership model

```rust
let radio_queue = RadioQueue::new();      // encoded TX FIFO
let radio_control = RadioControl::new();  // local retune requests
let radio = Radio::new(config, lr1121, &radio_queue, &radio_control)?;
radio.run_worker(None, on_packet).await    // takes ownership of `radio`
```

`Radio` borrows the queue and control objects. Once `run_worker` takes ownership, application code cannot call `Radio` methods; producers use the same `RadioQueue` handle and retunes use `RadioControl`. In this firmware both are static objects in `main.rs`, shared with the UI, diagnostics, and packet handler.

`Radio::send_packet` (usable only while `Radio` is still owned by the caller) checks the current band's encoded-size limit before enqueueing. After the worker starts, `RadioQueue::add_packet` validates packet metadata and the generic 255-byte limit but cannot know the active band limit; the worker rechecks the length at dequeue time and reports `PayloadTooLong`. Applications should therefore size fragments for the active band before enqueueing, which the UI send path does via `FrequencyBand::maximum_packet_size`.

## Queue behavior

| Property | Behavior |
| --- | --- |
| Capacity | 32 encoded packets, plus at most one in flight. |
| Ordering | FIFO. |
| Encoding | `add_packet` serializes a `PacketData` into a fixed 255-byte frame before enqueueing. |
| Backpressure | `QueueError::Full` / `RadioError::TxQueueFull`; the outbox keeps refused fragments and retries every 200 ms. |
| TX timeout | 30 seconds, reported as `TxTimeout`. |
| RX windows | After every two consecutive queued transmissions, RX is enabled for up to 500 ms before the next burst. S-band skips this. |
| Pacing | Each frame waits for the band's 10 % airtime budget before TX. |
| Retry / priority / persistence | The outbox retransmits an unacknowledged direct message after 20 s, up to three rounds. Queued frames are RAM-only. |
| Oversized frame | A frame larger than the current band's limit is logged and dropped, and the worker keeps running. |
| Duplicate filtering | Not performed on the TX queue; retries must reuse the original packet ID, so the queue must accept duplicates. |

Queueing a multi-fragment message is not atomic: `add_packet` returns `Full` when the FIFO cannot take the next fragment, and already-queued fragments cannot be rolled back. The current chat send path logs this and does not persist a sent record for the logical message.

## Worker loop

The worker is the only code that touches the LR1121 after startup. Its behavior:

1. On startup, start RX if the queue is empty and the band supports RX.
2. If a transmission is in flight, wait for an IRQ. `TX_DONE` clears the flag; if the queue is empty, RX restarts, otherwise the worker may open a listen window and continue. `TIMEOUT` clears the flag, restores RX if possible, and returns `TxTimeout`. Unrelated IRQs are ignored after clearing.
3. Otherwise `select` on the next queued packet, an RX IRQ, and a control-channel frequency request.
4. A queued packet: wait for the band's duty-cycle budget, read a pending RX packet if one is waiting, then suspend RX, load the LoRa payload or build the LR-FHSS frame, clear stale IRQs, and start TX.
5. An RX IRQ: read the payload length and bytes, clear the IRQ, restart RX, and await the packet callback. Header/CRC errors and non-RX-DONE results clear the IRQ and restart RX without a callback.
6. A frequency request: apply the retune only when no TX and no queued packets are pending.

Because the worker processes one operation at a time, RX pauses while TX and control operations are handled. An oversized RX payload, buffer-read failure, or SPI error propagates as a `RadioError` and ends the worker. `main.rs` logs the stopped/failed service, and the firmware's top-level `select` then finishes, leaving the remaining work idle; there is no supervisor that restarts the radio worker.

## Frequency changes

`RadioControl::request_frequency_change(hz)` validates that the frequency maps to a supported band and pushes one request into a one-slot channel. A second pending request returns `RadioBusy`; an unsupported frequency returns `InvalidConfiguration`. The worker applies accepted requests between operations.

Applying a retune selects the modem for the new band, recalibrates sub-GHz image rejection when the new frequency is below 1 GHz, sets the frequency and its RSSI calibration table, programs the modem/LoRa or LR-FHSS parameters, chooses a compatible PA mode, adjusts bandwidth and power when the new band cannot use the current values, republishes the new config, and resumes RX when the band supports it. If hardware reconfiguration fails, the worker restores the previous config before returning the error.

A local retune does not notify peers, and this firmware has no handler for the `FrequencyChange` packet type. Both operators must retune manually. The UI presets in `ui/state.rs` currently offer 869.4 MHz sub-GHz and 2.4 GHz; 2.4 GHz requires hardware that supports that path and a matching peer.

## LR1121 initialization

`Lr1121::new` performs a reset and BUSY handshake, wakes the chip, disables SPI CRC, selects the DCDC regulator, configures the board RF switch through DIO control (`RFSW0` receive, `RFSW1` transmit), enables a 3.0 V TCXO, starts the 32.768 kHz LF clock, and runs full calibration.

`Radio::new` then validates the config and applies the band: standby, packet-type selection, sub-GHz image calibration (±10 MHz around the center in 4 MHz steps), RF frequency, RSSI calibration, LoRa or LR-FHSS parameters, sync word, PA configuration, TX power with a 48 µs ramp, RC-standby fallback, RX-boost disabled, cleared IRQs, and TX/RX/error/timeout IRQs routed to DIO9. The driver sends command plus parameters in one NSS frame and uses a second frame for read responses; status reads use the direct status read.

The driver implements the startup sequence this board needs, not the LR1121's complete command set. Consult [`62740776.LR1121_UM_V2.2.pdf`](../62740776.LR1121_UM_V2.2.pdf) for register-level behavior.

## Idle traffic

The firmware queues no packet on its own. The only unsolicited transmission is the `DeviceAnnounce` broadcast produced by the contacts screen's ADD DEVICE scan: plaintext, empty payload, hop limit 3, repeated every two seconds while the scan is open and stopped when the window closes or BACK is pressed. Chat, coordinate, ACK, pairing, and add-contact-request packets are sent only in response to a user action or an incoming authenticated message.

## Verifying a link on hardware

Bench bring-up requires actual hardware and cannot be inferred from the source:

1. Confirm both boards are on the same frequency, bandwidth, spreading factor, coding rate, preamble, header mode, CRC setting, and sync word.
2. Confirm the antenna is connected and supports the band in use before transmitting at high power.
3. Watch the RX log line emitted for accepted frames; it reports sender, type, IDs, encryption, routing classification, hop state, and size.
4. For encrypted direct traffic, confirm a matching contact exists on both ends; without it, the encrypted packet is rejected and no chat row appears.
5. Use the ACK indicator to confirm end-to-end storage acceptance, and remember it does not prove display.

There is no recorded two-board range test, RF-switch measurement, or regulatory certification in this repository.