# Packets and Radio

## Can I Send and Receive Now?

At the firmware level:

| Band | TX | RX | Notes |
| --- | --- | --- | --- |
| Sub-GHz LoRa, 150-960 MHz | Yes | Yes | The default `main.rs` profile is 869.4 MHz, SF9, 125 kHz, CR 4/5, 22 dBm, explicit header, CRC on. |
| 2.4 GHz LoRa, 2.4-2.5 GHz | Yes | Yes | Requires an explicit 2.4 GHz `RadioConfig`; it is not the current `main.rs` default. |
| S-band LR-FHSS, 1.9-2.1 GHz | Yes | No | The LR1121 User Manual states its LR-FHSS implementation is transmit-only. S-band also has no LoRa mode, so S-band RX is rejected. |

The current `main.rs` starts RX and queues a broadcast `Text` packet every 30 seconds. Use two boards with the same frequency, spreading factor, bandwidth, coding rate, preamble, header mode, CRC setting, and sync word. The firmware sets the private LoRa sync word to `0x12`; the public LoRaWAN value is `0x34`, so LoRaWAN devices will not decode these packets.

This is a code-level capability statement, not a hardware validation. No two-board RF test is recorded in this repository. The build passes, but antenna connection, RF-switch behavior, range, and peer interoperability still need bench verification.

## Sync Words vs. Your Packet Decoder

The sync word is handled inside the LR1121 PHY. It is **not** part of the 43-byte protocol packet, it is not written by `PacketEncoder`, and it is never passed to `PacketDecoder`.

Transmit path:

```text
PacketEncoder -> LR1121 SPI buffer -> preamble + sync word + LoRa header + payload + CRC
```

Receive path:

```text
preamble + sync word matched by LR1121 -> header/payload demodulated
-> RX_DONE or error IRQ -> firmware reads bytes over SPI -> PacketDecoder
```

Consequences:

- The sync word matters even when you implement your own packet decoder. Your decoder only runs after the LR1121 has recognized the frame and delivered bytes over SPI. If the transmitter and receiver use different sync words, the LR1121 normally never raises `RX_DONE`, so `PacketDecoder` is never called and cannot recover the packet.
- The sync word does not alter, encrypt, authenticate, or address the payload bytes. Your decoder sees the same custom packet bytes regardless of which sync word carried them.
- If you decode raw IQ from an SDR, you can choose to ignore the sync word in your own software, but the sync word is still present in the waveform. A LoRa modem receiver must be configured with the same value.
- A sync word is a network filter, not a secret. It reduces accidental cross-network reception but provides no security.

The LR1121 has separate sync-word settings for each modem. This firmware does **not** use all three at once; only the setting for the currently selected packet type matters:

| Modem | Sync word size | Value used by this firmware | Where it is used |
| --- | ---: | --- | --- |
| LoRa | 1 byte | `0x12` (private) | Sub-GHz and 2.4 GHz LoRa; set during radio configuration. `0x34` is the public LoRaWAN value and is not used. |
| LR-FHSS | 4 bytes | `0x2C0F7995` | S-band LR-FHSS TX; set again immediately before building the frame. |
| GFSK | 8 bytes | Not configured | GFSK is not used by this firmware. |

So: use `0x12` for LoRa links, use `0x2C0F7995` for LR-FHSS links, and ignore the GFSK setting unless GFSK support is added later.

## Wire Format

Packets use one format on every RF band. The current header is 43 bytes; payload begins at byte 43. All multi-byte integers are big-endian.

| Field | Offset | Size | Meaning |
| --- | ---: | ---: | --- |
| `version` | 0 | 1 | Protocol version, currently `1`. |
| `packet_id` | 1 | 2 | Packet identifier. |
| `message_id` | 3 | 2 | Shared identifier for fragments of one message. |
| `fragment_index` | 5 | 1 | Zero-based fragment index. |
| `fragment_count` | 6 | 1 | Number of fragments; must be nonzero. |
| `from` | 7 | 16 | Sender device ID. |
| `to` | 23 | 16 | Recipient ID; all zeros mean broadcast. |
| `is_encrypted` | 39 | 1 | `0` or `1`. This flag does not perform encryption. |
| `packet_type` | 40 | 1 | Application type byte. |
| `initial_hop_limit` | 41 | 1 | Original hop limit. |
| `hop_limit` | 42 | 1 | Current hop limit. |
| `payload` | 43 | Variable | Application bytes, or intended encrypted payload if crypto is implemented. |

Current packet type assignments:

| Value | Variant |
| ---: | --- |
| `0x01` | `Text` |
| `0x02` | `Ack` |
| `0x03` | `Coordinates` |
| `0x04` | `Heading` |
| `0x05` | Unassigned |
| `0x06` | `FrequencyChange` announcement type; no handler yet |

`PacketData::new` validates fragment metadata, payload capacity, and the minimum nonce/tag length when `is_encrypted` is true. It does not encrypt, authenticate, fragment, or reassemble data.

## Construct and Send

Create a `PacketData` value with IDs, addressing, type, and payload, then enqueue it using `Radio::send_packet`:

```rust
use crate::packet::{packet_data::{DEFAULT_HOP_LIMIT, PacketData}, packet_type::PacketType};
use crate::radio::radio::Radio;

let packet = PacketData::new(
    packet_id,
    message_id,
    0,                     // fragment index
    1,                     // fragment count
    sender_id,
    recipient_id,          // [0; 16] is broadcast
    false,                 // no encryption is performed by the current code
    PacketType::Text,
    b"hello",
    DEFAULT_HOP_LIMIT,
    DEFAULT_HOP_LIMIT,
)?;

radio.send_packet(&packet)?;
```

`Radio::send_packet` validates the packet's complete encoded length against the active band's limit, then `radio::queue` encodes it and places the raw bytes in a static Embassy channel with capacity 4. The method returns `TxQueueFull` if the queue is full. `queue::add_packet` is also used in `main.rs`, but it only enforces the generic 255-byte packet limit; prefer `Radio::send_packet` for band-aware validation.

`run_worker` consumes the `Radio` value and then services the queue and IRQs. A basic usage shape is:

```rust
radio.send_packet(&packet)?;
radio.run_worker(None, |encoded_packet| {
    PacketHandler::handle_packet(encoded_packet);
}).await?;
```

Pass `None` for explicit-header LoRa RX. Implicit-header RX requires `Some(payload_len)` and a matching implicit-header configuration. The worker currently runs indefinitely unless a radio error occurs.

## Receive Path

The worker reads RX buffer status and bytes, clears the interrupt, and passes a raw encoded packet to the callback. `PacketHandler::handle_packet` decodes the packet, prints its type/IDs/hop count, and prints the payload as UTF-8 or hexadecimal bytes. Invalid packets are logged without panicking. The empty `packet_router` module does not forward or duplicate-suppress packets.

RX is single-packet mode (`set_rx(0)`), and the worker restarts RX after each packet or error. Header errors, CRC errors, and non-RX IRQs clear the pending IRQ and restart RX. TX uses a 30-second radio timeout and reports `TxTimeout` on expiry.

## LR1121 Hardware Setup

`Lr1121::new` performs these steps before `Radio::new` runs:

- Hard reset, BUSY wait, and NSS wakeup pulse.
- SPI CRC disabled, matching the board reference HAL.
- DCDC regulator selected.
- Board RF switch configured with `SetDioAsRfSwitch` (`RFSW0 | RFSW1`; RX on RFSW0, TX on RFSW1).
- 3.0 V TCXO with a 300-tick startup time (30.52 us steps).
- 32.768 kHz crystal LFCLK, waiting for ready.
- XOSC standby and full calibration (`0x3F`), then error/IRQ clearing.

`Radio::new` then applies the selected band:

- Enters XOSC standby and selects the LoRa or LR-FHSS packet type.
- For sub-GHz, calibrates image rejection over the configured frequency +/-10 MHz in 4 MHz steps.
- Sets RF frequency and the matching RSSI calibration table.
- Configures LoRa modulation/packet parameters and private sync word `0x12`, or the LR-FHSS modulation profile.
- Configures the PA, TX power, and 48 us ramp.
- Selects RC-standby fallback, disables RX boost, clears IRQs, and routes TX/RX/error/timeout IRQs to DIO9.

The driver follows the board reference SPI sequence: command+parameters are sent in one NSS frame; read commands use a second NSS frame for the status byte and response. `GetStatus` is a direct six-byte read with no opcode.

## Radio Construction

The LR1121 driver is built from the ESP-HAL SPI bus and four pins. `Lr1121::new` performs the hard reset and system startup described above. `Radio::new` validates `RadioConfig`, applies the per-band frequency/modulation/packet/PA/TX settings, clears IRQs, and routes radio IRQs to DIO9.

Example profile from `main.rs`:

```rust
let radio_config = RadioConfig {
    frequency_band: FrequencyBand::SubGhz,
    bandwidth: LoRaBandwidth::Khz125,
    pa_mode: PaMode::HighPower,
    tx_power_dbm: 22,
    ..RadioConfig::new(869_400_000)
};

let radio = Radio::new(radio_config, lr1121)?;
```

`RadioConfig::new(frequency_hz)` selects a band from the frequency and supplies defaults. Set the band-specific fields explicitly when the default is not the desired profile. `validate` rejects out-of-band frequencies, unsupported bandwidth/band combinations, PA mismatches, invalid power, spreading factors outside 5-12, and a zero preamble.

## Bands and Limits

These are LR1121 silicon ranges and firmware modem choices, not a guarantee that a particular board has a suitable RF path.

| `FrequencyBand` | Range | Modem | RX | Maximum complete encoded packet |
| --- | --- | --- | --- | ---: |
| `SubGhz` | 150-960 MHz | LoRa | Yes | 255 bytes |
| `Sband` | 1.9-2.1 GHz | LR-FHSS | No; TX only | 133 bytes |
| `Ism2_4Ghz` | 2.4-2.5 GHz | LoRa | Yes | 255 bytes |

The common packet header remains 43 bytes. Thus the packet payload space is 212 bytes on the 255-byte profiles and 90 bytes on the current 133-byte LR-FHSS profile. If the intended AES-GCM format is later implemented, its 12-byte nonce and 16-byte tag reduce application data capacity to 184 and 62 bytes respectively. These are mathematical limits, not implemented encryption limits.

LoRa bandwidth is represented by `LoRaBandwidth`. Sub-GHz supports 62.5, 125, 250, and 500 kHz in the current config. 2.4 GHz additionally maps 203, 406, and 812 kHz to the band-specific LR1121 encodings. S-band ignores the LoRa modulation fields and uses `LrFhssProfile::SbandDefault`.

PA settings are represented by `PaMode`: LP (`LowPower`, up to +14 dBm), HP (`HighPower`, up to +22 dBm), and HF (`HighFrequency`, up to +13 dBm). On this board, both sub-GHz modes program the HP path (`pa_select=1`, `supply=1`, `duty_cycle=0x04`, `hp_setup=0x07`); the HF path uses `(2, 0, 0x07, 0)`. These are chip-path limits. Board power, thermal, antenna, and regional limits may be lower; the 22 dBm main profile must not be treated as a certification.

The current S-band LR-FHSS profile uses GMSK, 136.72 kHz bandwidth, 5/6 coding rate, 3906 Hz non-FCC grid spacing, two headers, enabled hopping, and sync word `0x2C0F7995`. The LR1121 builds the LR-FHSS frame from the encoded protocol packet. The profile accepts at most 133 input bytes and cannot receive.

## Live Retuning

`Radio::set_frequency(frequency_hz)` retunes directly only when the radio is idle; it returns `RadioBusy` while TX is active or packets remain queued. If RX was active, it stops RX, reconfigures the band/modem/PA, and resumes RX where supported.

The worker owns the radio after `run_worker` begins. From another task, call the module function to enqueue a local retune request:

```rust
use crate::radio::radio::request_frequency_change;

request_frequency_change(2_450_000_000)?;
```

The request channel holds one pending change. The worker applies it between operations. This function changes only this device's local radio; it does not transmit a `FrequencyChange` packet or coordinate peers. `PacketType::FrequencyChange` is currently just a wire enum variant.

The firmware now configures the LR1121 RF switch, TCXO, sub-GHz image calibration, and per-frequency RSSI calibration. Before using a non-default band, still verify the board's antenna/front end, actual supported frequency range, and local regulations. In particular, the Rust S-band range starts at 1.9 GHz, while the Waveshare board reference uses 2.0-2.1 GHz; keep both ends of a link inside a range the hardware actually supports.

For a first link test, use two boards on sub-GHz with the same configuration and watch the serial console for `RX ...` lines. The current test producer broadcasts from device ID `[1; 16]`, so no addressing or encryption setup is required to observe the test payload.