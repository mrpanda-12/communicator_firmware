# Communicator Protocol Specification

## 1. Packet Format

Each radio packet contains a fixed header followed by a variable-length body. The version field is the first byte. The total received packet length determines the body length; there is no separate payload-length field.

| Field | Size | Description |
| --- | --- | --- |
| version | 1 byte | Protocol version. Initial version is 1. |
| packet_id | 2 bytes | Packet identifier unique per sender. Retries reuse this fragment’s ID. |
| message_id | 2 bytes | Identifier shared by all fragments of one logical message. Unique per sender while in transit. |
| fragment_index | 1 byte | Zero-based fragment index, from 0 to fragment_count - 1. |
| fragment_count | 1 byte | Total number of fragments, from 1 to 255. |
| from | 16 bytes | Sender’s long device ID. |
| to | 16 bytes | Recipient’s long device ID. All zero bytes indicate broadcast. |
| is_encrypted | 1 byte | 0 = unencrypted; 1 = encrypted. Other values are invalid. |
| type | 1 byte | Message type. Readable in the header, including for encrypted packets. `0x06` identifies a frequency-change announcement. |
| initial_hop_limit | 1 byte | Original maximum additional relays allowed. |
| hop_limit | 1 byte | Remaining relays allowed. Mutable by relays. |
| payload | Var | The message contents, formatted structurally based on the is_encrypted flag as specified in Section 5. |

The header is 43 bytes. A single-packet message uses fragment_index = 0 and fragment_count = 1.

Receivers must reject unsupported versions, invalid field values, and packets too short to contain the required header and security fields. All multi-byte integers use big-endian byte order.

## 2. Device ID and Key Generation (Hardware TRNG)

Generate device IDs and nonces using a hardware true random source. Do not treat uptime or CPU clock values as cryptographic entropy. On ESP32-S3, use the HAL TRNG only while a supported entropy source is active, such as its ADC source or RF subsystem.

1. Device ID Generation: The intended behavior is to generate a 16-byte Device ID during initial setup and persist it across restarts. The current identity module provides explicit load and regenerate operations, but `main.rs` does not initialize or call it. Regenerate only after an explicit user action. A new ID requires pairing again.
2. Unique Peer Key Derivation: When pairing two devices, an exclusive, unique symmetric encryption key (Key_A_B) is derived using a Key Derivation Function (KDF) like HKDF, combining the global Secret AES Password and both high-entropy hardware-generated Device IDs. Names are strictly excluded from the derivation:
   `Key_A_B = HKDF(Secret_Password, ID_A || ID_B)`
   The global Secret AES Password is configured out-of-band and never transmitted over the air.

3. Pairing, Identity Verification, and Rekeying

Pairing is an out-of-band or secure local process where friends exchange their hardware-generated 16-byte Device IDs.

- Each device stores known contacts as a map of device_id to their corresponding derived unique peer key (Key_A_B).
- A known device is authenticated exclusively when the AES-256-GCM authentication tag verifies successfully using that specific peer key.
- Identity Regeneration (Rekeying): If a user explicitly generates a new identity, a new device_id is computed via the TRNG. Because the unique peer keys are cryptographically bound to the node's own ID through the HKDF step, all existing derived keys become instantly invalid. The node must re-initiate the pairing process and re-exchange its new Device ID with all contacts to calculate the new valid symmetric keys. Existing stored keys of friends remain intact but unusable until mutual pairing is completed with the new ID.

## 4. Fragmentation and Reassembly

This section describes intended behavior. Packet metadata is represented in code, but fragment splitting, reassembly, timeout handling, and authenticated fragment acceptance are not implemented.

A message that does not fit in one radio packet is split into fragments. Every fragment is a separate packet with its own packet_id and—if encrypted—its own unique nonce and authentication tag. All fragments of a message share the same from and message_id.

The sender sets fragment_count to the total number of fragments and numbers them from zero. Fragment data is concatenated in index order. Each fragment must fit the configured physical radio transmission limits including its header and security overhead.

Receivers assemble fragments using (from, message_id). Before accepting a fragment, verify its authentication tag. Discard incomplete messages after a timeout. Implementations must enforce strict limits on memory allocation based on an untrusted fragment_count.

## 5. Payload Format and Cryptographic Structure

The layout of the variable-length payload field depends strictly on the value of the is_encrypted header flag.

### Case A: Encrypted Packets (is_encrypted = 1)

This is the intended wire format. `crypto/mod.rs` provides AES-256-GCM `derive_key`, `encrypt`, and `decrypt` primitives, but the packet encode/decode path does not call them yet.

Encrypted packets use AES-256-GCM. For each transmitted fragment, the sender generates a unique 12-byte Nonce using the ESP32-S3 hardware TRNG. This nonce is transmitted directly within the packet payload to ensure the receiver can instantly decrypt the frame without any state synchronization.

Wire Layout for the payload Field:

`payload = nonce (12 bytes) || ciphertext (encrypted data) || authentication_tag (16 bytes)`

The GCM plaintext consists purely of the raw application data. All immutable header fields (everything except hop_limit) are passed as Additional Authenticated Data (AAD) into the AES-GCM engine. This guarantees absolute header integrity while permitting mesh relays to decrement the hop_limit without breaking the authentication tag.

The receiver must extract the 12-byte nonce first, then verify the 16-byte authentication tag before acting on or processing decrypted data. For a retry, the sender resends the exact encoded fragment with its original packet_id, ciphertext, and nonce.

### Case B: Unencrypted Packets (is_encrypted = 0)

When encryption is disabled, no cryptographic transforms are applied, and no encryption tags or nonces are appended to the transmission frame.

Wire Layout for the payload Field:

`payload = data`

The payload contains the raw, unmodified application data matching the fragment window.

## 6. Physical Layer & Regional Transmission Profiles (Germany / ETSI)

The regional labels and numeric limits below are design targets, not legal advice or regulatory certification. The current firmware does not implement duty-cycle enforcement or select a regional profile automatically. Validate the applicable rules and board limits for the deployment region.

### LR1121 RF Bands

The packet header, payload structure, encryption fields, and packet-type values are identical on every frequency band. Only the maximum encoded packet size is modem-dependent; senders must keep the complete encoded packet within the selected profile's limit.

The LR1121 frequency ranges and modem capabilities used by the firmware are:

| Frequency band | Center-frequency range | Firmware modem | Direction | Maximum encoded packet |
| --- | --- | --- | --- | --- |
| Sub-GHz | 150–960 MHz | LoRa | Transmit and receive | 255 bytes |
| S-band (2 GHz) | 1.9–2.1 GHz | LR-FHSS | Transmit only | 133 bytes |
| 2.4 GHz ISM | 2.4–2.5 GHz | LoRa | Transmit and receive | 255 bytes |

The 2.4 GHz LoRa bandwidth encodings include the band-specific 203, 406, and 812 kHz values as well as the shared LoRa bandwidths. S-band LR-FHSS uses the LR1121 frame builder and cannot receive; a worker remains TX-capable but does not start RX while configured in S-band. The HF PA path is selected for S-band and 2.4 GHz and is limited here to +13 dBm. Sub-GHz uses the LP or HP PA path according to configuration.

`Radio::set_frequency` changes bands while the radio is idle. While the worker owns the radio, call `radio::radio::request_frequency_change`; the worker applies the request between TX/RX operations. The target frequency must be in one of the ranges above. Changing a frequency does not tell another device to follow.

`FrequencyChange` is packet type `0x06` for an announcement/invitation such as “I’m changing frequency; do you want to join me?”. It has no handler or automatic retune behavior in this firmware.

These are LR1121 silicon capabilities, not a guarantee that a particular board supports each band. The firmware configures the LR1121 RF-switch DIO control, TCXO/LFCLK startup, sub-GHz image calibration, and per-frequency RSSI calibration. The attached antenna and front end must still support the selected band. Verify the board RF path and local regulations before transmitting outside the existing sub-GHz profile.

The packet format is shared across bands. The radio layer limits the complete encoded packet to the selected modem profile's maximum. The caller must create each fragment; automatic fragmentation and reassembly are not currently implemented.

### Profile A: Sub-GHz Long-Range Mesh Mode (869.5 MHz)

- Regulatory Environment: Governed by ETSI EN 300 220 (Sub-band P). Max transmit power is 22 dBm (160 mW). Duty-cycle limit is 10% (360 seconds of transmission per hour).
- Optimization Strategy: To minimize "Time-on-Air" and prevent legal duty-cycle lockout under low Spreading Factors, packets are optimized for rapid bursts.
- Target Payload Size: 100 Bytes of application data per fragment (design target, not enforced by the current packet code).
- Total Over-The-Air Footprint: 43B Header + 12B Nonce + 100B Ciphertext + 16B Tag = 171 Bytes total.

### Profile B: 2.4 GHz Ultra-Fast LoRa Mode (2400 MHz)

- Regulatory Environment: Governed by ETSI EN 300 328. Max transmit power is 10–20 dBm (dependent on exact modulation width). 0% Fixed Duty Cycle (No transmission timing lockouts).
- Optimization Strategy: Due to the wide bandwidth (up to 1.6 MHz) and rapid symbol speeds, transmission overhead is negligible. The physical hardware limits of the Semtech LR1121 FIFO buffer are maxed out.
- Target Payload Size: 184 Bytes of application data per fragment (maximum if AES-GCM overhead is later implemented).
- Total Over-The-Air Footprint: 43B Header + 12B Nonce + 184B Ciphertext + 16B Tag = 255 Bytes total (absolute encoded-packet limit).

## 7. Transmission Queue, Forwarding, and Duplicate Detection

### Transmission Queue

This subsection describes implemented behavior.

All outgoing packets use the radio worker's FIFO queue. `Radio::send_packet` checks the encoded packet against the active band limit and enqueues it. After the worker owns `Radio`, other tasks use `queue::add_packet`, which validates the packet and the generic 255-byte limit but not the active band limit; the worker rechecks the limit at dequeue time.

The queue has capacity 16 encoded packets (plus at most one in-flight packet) and provides backpressure through `TxQueueFull`. After every two queued transmissions, the worker opens an RX window of up to 500 ms before continuing; S-band skips this because it is TX-only. The queue does not implement priority, preemption, emergency bypass, retries, persistence, or duplicate suppression. `Radio::start_transmit` is private, so there is no public `Radio` direct-send path. The low-level `Lr1121` driver commands are public but are not a supported bypass because they skip worker pacing, RX suspension/resumption, and TX state tracking. A future priority or emergency path must be an explicit API that still uses the radio worker and does not skip duplicate handling.

Duplicate suppression must not be applied when enqueuing: retries intentionally reuse `packet_id`, and a retry must remain queueable after the original transmission is lost. Duplicate detection belongs on the receive/forwarding path.

### Forwarding and Duplicate Detection

This subsection describes intended behavior. The current `network::packet_router` module is empty, and the packet handler does not forward or deduplicate packets.

For each received packet:

1. Validate its structure and protocol version.
2. Check the duplicate cache using (from, packet_id). Discard packets already seen.
3. Process the packet locally if to matches this device’s ID or is a broadcast address.
4. If hop_limit is zero, do not forward it.
5. Otherwise decrement hop_limit and forward, without changing any other field.

The initial hop limit is 3. Packets addressed to the receiving device are processed locally and are not forwarded by default. The hop_limit is the only mutable header field and is excluded from the AEAD AAD tracking.

## 8. Persistent Storage

Store the local Device ID, the master Secret AES Password, known contacts, and the replay-protection window across restarts. The local Device ID is loaded or regenerated through `device::identity`; regeneration is an explicit operation and requires pairing again.

### Flash Store

`storage::flash::FlashStore` provides persistent typed settings and contact records on ESP flash. It wraps `esp_storage::FlashStorage` for async use and delegates page management and wear leveling to `sequential-storage`.

Construct the store with the ESP flash peripheral and the absolute flash address range reserved for application data:

```rust
let mut store = FlashStore::new(peripherals.FLASH, data_start..data_end)?;
```

The range must belong to a dedicated data partition, start and end on flash erase-page boundaries, and contain at least two pages. `MapConfig::try_new` checks the range geometry, but does not check the device partition table or prevent overlap with firmware. Writes can erase sectors inside the supplied range. Never use a range occupied by the bootloader, partition table, firmware, or another data owner. The project must obtain `data_start..data_end` from its actual partition layout; no safe default range is currently defined.

Settings use named keys and typed values. Keep the value type for a setting name consistent, because the stored record does not include a type tag. Names are limited to 31 UTF-8 bytes. Supported primitive values include booleans, signed and unsigned integers, and `f32`/`f64`.

```rust
store.set("display_brightness", &80u8, &mut buffer).await?;
let brightness = store.get::<u8>("display_brightness", &mut buffer).await?;
```

Contacts are keyed by their 16-byte device ID and saved with `write_contact`; `read_contact` retrieves one by ID. Their serialized record contains the ID, a 32-byte derived peer key, and a 32-byte custom-name field. The key is stored without application-level encryption.

Provide a work buffer large enough for the largest serialized key and value, rounded up to the flash word size. For a contact record, use at least 160 bytes. Keep a settings buffer large enough for the largest setting key and value you store.
