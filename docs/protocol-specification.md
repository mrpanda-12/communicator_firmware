# Communicator Protocol Specification

> **Status:** This document describes both current behavior and protocol constraints. Passive discovery and one-shot announcements, shared-secret pairing, pair-key persistence, AES-GCM protection for paired direct text and ACKs, bounded text fragmentation/reassembly, accepted-message ACK generation, flash-backed chat storage with persisted ACK state, and received-coordinate navigation are implemented. Discovery and unpaired introductions are intentionally unauthenticated/plaintext.

## 1. Packet Format

Each radio packet contains a fixed header followed by a variable-length body. The version field is the first byte. The total received packet length determines the body length; there is no separate payload-length field.

| Field | Size | Description |
| --- | --- | --- |
| version | 1 byte | Protocol version. Initial version is 1. |
| packet_id | 2 bytes | Packet identifier unique per sender. Retries reuse this fragment’s ID. |
| message_id | 2 bytes | Identifier shared by all fragments of one logical message. Unique per sender while in transit. |
| fragment_index | 1 byte | Zero-based fragment index, from 0 to fragment_count - 1. |
| fragment_count | 1 byte | Total number of fragments, from 1 to 16 in this firmware. |
| from | 16 bytes | Sender’s long device ID. |
| to | 16 bytes | Recipient’s long device ID. All zero bytes indicate broadcast. |
| is_encrypted | 1 byte | 0 = unencrypted; 1 = encrypted. Other values are invalid. |
| type | 1 byte | Message type. Readable in the header, including for encrypted packets. `0x02` is an ACK, `0x06` is a frequency-change announcement, `0x07` a device-announce response, and `0x08` a pairing-confirmation frame. |
| initial_hop_limit | 1 byte | Original maximum additional relays allowed. |
| hop_limit | 1 byte | Remaining relays allowed. Mutable by relays. |
| payload | Var | The message contents, formatted structurally based on the is_encrypted flag as specified in Section 4. |

The header is 43 bytes. A single-packet message uses fragment_index = 0 and fragment_count = 1.

Receivers must reject unsupported versions, invalid field values, and packets too short to contain the required header and security fields. All multi-byte integers use big-endian byte order.

## 2. Device IDs, Pair Keys, and Pairing

Generate device IDs and nonces using a hardware true random source. Do not treat uptime or CPU clock values as cryptographic entropy. On ESP32-S3, use the HAL TRNG only while a supported entropy source is active, such as its ADC source or RF subsystem.

1. Device ID Generation: Generate a 16-byte device ID using the hardware TRNG and persist it across restarts. Device IDs are public identifiers, not secrets or proof of identity.
2. Pair-key derivation: Both people enter the same strong, high-entropy secret locally (on the current keypad, two hex digits type one ASCII character). The devices sort the two public IDs and derive a 32-byte key using PBKDF2-HMAC-SHA256 (100,000 iterations) with `communicator-pair-v2 || lower_id || higher_id` as salt/context. The secret is never sent. Use at least 16 random characters (or an equivalently high-entropy phrase); a short password remains vulnerable to offline guessing despite the KDF.
3. Authenticated confirmation: The lower device ID sends a proof, the higher ID returns a fresh challenge, and the lower ID sends a final proof bound to both nonces. Each frame uses AES-256-GCM authentication over its immutable packet context. A device persists the keyed contact only after verifying the relevant authenticated proof; captured old frames cannot satisfy a new challenge exchange.
4. A saved contact maps the peer's public device ID to its 32-byte derived key. Identity regeneration changes the KDF context, so users must pair again with the new ID.

## 3. Fragmentation and Reassembly

The sender splits logical text up to 512 bytes into at most 16 fragments. Each radio packet has its own `packet_id` (`base_packet_id + fragment_index`) while every fragment shares the logical `message_id`, sender, recipient, and packet type. Fragment indices start at zero and the payload is joined in index order.

The fixed header is 43 bytes. The sender uses the active band's encoded-packet limit when choosing each fragment size. On the 255-byte LoRa profiles, plaintext fragments carry up to 212 application bytes and AES-GCM fragments up to 184; on the 133-byte S-band profile, these limits are 90 and 62 bytes. Each encrypted fragment has its own 12-byte nonce and 16-byte tag. Encrypt each fragment independently after its fragment metadata is final, so the tag authenticates the fragment index/count and other immutable header fields.

The receiver authenticates/decrypts each encrypted fragment before adding its plaintext to a bounded reassembly entry. It accepts no more than 16 fragments and 512 assembled bytes, checks that fragments agree on sender/message/recipient/type/encryption/original hop limit, and delivers or acknowledges text only after the complete UTF-8 message is persisted to chat storage. Four incomplete assemblies are retained; when that cache is full, the oldest entry is evicted. There is no time-based retry or reassembly guarantee, so a missing fragment leaves no delivered message or ACK. Relays forward fragments independently without decrypting them.

If any fragment fails authentication, metadata consistency, size, or UTF-8 validation, do not display the logical message or generate its ACK. Duplicate packet IDs are filtered before reassembly; identical retries of already accepted fragments may trigger another ACK without duplicating the chat item.

## 4. Payload Format and Cryptographic Structure

The layout of the variable-length payload field depends strictly on the value of the is_encrypted header flag.

### Case A: Encrypted Packets (is_encrypted = 1)

Paired direct `Text` packets use AES-256-GCM. Discovery/control packets remain unencrypted where specified, and a user may explicitly send an unencrypted introduction before pairing.

Encrypted packets use AES-256-GCM. For each encrypted packet, the sender generates a 12-byte nonce using the ESP32-S3 random source. This nonce is transmitted directly within the packet payload so the receiver can verify/decrypt without prior nonce synchronization.

Wire Layout for the payload Field:

`payload = nonce (12 bytes) || ciphertext (encrypted data) || authentication_tag (16 bytes)`

The GCM plaintext consists purely of the raw application data. AAD contains the encoded immutable header fields: version, packet ID, message ID, fragment index/count, sender ID, recipient ID, encryption flag, packet type, and `initial_hop_limit`. The mutable `hop_limit` is excluded so relays can decrement it without invalidating the tag. Receivers use the sender's stored contact key; missing keys, invalid tags, and plaintext from a paired contact are rejected. This authenticates packet context but does not hide IDs or header fields.

The receiver must extract the 12-byte nonce first, then verify the 16-byte authentication tag before acting on or processing decrypted data. For a retry, the sender resends the exact encoded fragment with its original packet_id, ciphertext, and nonce.

### Direct-message acknowledgements (`Ack`, type `0x02`)

An `Ack` is queued only for an encrypted direct message: the packet must be unicast (never broadcast) and must have been authenticated with the sender's stored contact key, and its decrypted payload must have passed validation and been persisted. Plaintext introductions, unauthenticated coordinates, and broadcast traffic are delivered without any acknowledgement.

The ACK application payload is exactly 34 bytes and names the acknowledged message: the original sender's 16-byte device ID, the original recipient's 16-byte device ID, and the logical `message_id` as two big-endian bytes. The ACK header repeats that `message_id` and carries the acknowledging device as `from` and the original sender as `to`. The payload is AES-256-GCM protected with the contact key and the ACK packet's immutable header as AAD. A receiver rejects an ACK it cannot authenticate, and rejects one whose payload does not name this device as the original recipient and the ACK sender as the original sender. An identical retry of an already accepted direct packet may receive another ACK without creating another chat entry.

An ACK does not prove the message was rendered or read. The chat UI marks a matching encrypted direct-message row with a green check only after receiving the authenticated ACK from that peer; the acknowledged flag is then written to the sender's stored message record, so the marker persists across reboots. Plaintext introductions and broadcast rows never get this indicator. The radio queue does not automatically retry transmissions, and ACK packet queueing can fail if the transmit queue is full.

### GPS coordinates (`Coordinates`, type `0x03`)

A coordinate payload is exactly eight bytes: signed 32-bit latitude microdegrees followed by signed 32-bit longitude microdegrees, both big-endian. Valid ranges are `-90_000_000..=90_000_000` and `-180_000_000..=180_000_000`. The receiver may accept an unencrypted coordinate from an unpaired sender, but paired contacts must send authenticated encrypted packets; malformed coordinates are discarded. An unencrypted coordinate is displayed without an acknowledgement. A valid received coordinate appears as a selectable `GPS` row in the matching conversation. The UI currently receives coordinates but does not expose a command to transmit the local position.

Select the GPS row with Up/Down and press Enter to open navigation. The screen calculates great-circle distance and absolute target bearing from the latest local GPS fix. It estimates the user's course from successive fixes after at least five metres of movement, then reports a left/right/straight turn toward the target. No compass sensor is available. GPS drift, stale fixes, low movement, and poor satellite geometry can make the course estimate unavailable or noisy; the screen asks the user to move to establish direction and does not provide turn-by-turn road routing.

### Case B: Unencrypted Packets (is_encrypted = 0)

When encryption is disabled, no cryptographic transforms are applied, and no tags or nonces are appended. This applies to broadcast/discovery traffic and to the explicitly warned unpaired introduction path; do not use it for a paired contact.

Wire Layout for the payload Field:

`payload = data`

The payload contains the raw, unmodified application data matching the fragment window.

## 5. Physical Layer & Regional Transmission Profiles (Germany / ETSI)

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

`Radio::set_frequency` changes bands while the radio is idle and before the worker takes ownership. After that, request a local retune with `RadioControl::request_frequency_change` on that radio's control channel. The call validates the software frequency range and enqueues one pending request; it does not perform the hardware operation synchronously. A second pending request returns `RadioBusy`, and an out-of-range value returns `InvalidConfiguration`. The worker applies an accepted request between radio operations, reconfigures the modem and PA for the target band, and resumes RX if the radio was receiving and the new band supports RX. S-band is TX-only. A hardware failure is returned by the worker's `run_worker` future. Create one `RadioControl` per radio and pass it to `Radio::new`. A local retune does not notify or retune any peer.

`FrequencyChange` is packet type `0x06` for an announcement/invitation such as “I’m changing frequency; do you want to join me?”. It has no handler or automatic retune behavior in this firmware.

These are LR1121 silicon capabilities, not a guarantee that a particular board supports each band. The firmware configures the LR1121 RF-switch DIO control, TCXO/LFCLK startup, sub-GHz image calibration, and per-frequency RSSI calibration. The attached antenna and front end must still support the selected band. Verify the board RF path and local regulations before transmitting outside the existing sub-GHz profile.

The packet format is shared across bands. The sender sizes fragments to the selected modem profile's maximum complete encoded packet; the receiver reassembles at most 16 fragments and 512 application bytes. S-band's smaller packet limit therefore requires more fragments than the LoRa profiles for the same logical text.

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

## 6. Transmission Queue, Forwarding, and Duplicate Detection

### Transmission Queue

This subsection describes implemented behavior.

Each radio uses its own `RadioQueue` FIFO channel. `Radio::send_packet` checks the encoded packet against the active band limit and enqueues it. After the worker owns `Radio`, other tasks use that radio's `RadioQueue::add_packet`, which validates the packet and the generic 255-byte limit but not the active band limit; the worker rechecks the limit at dequeue time. Create one `RadioQueue` for each `Radio` and pass that same queue to its producers and packet forwarder.

The queue has capacity 16 encoded packets (plus at most one in-flight packet) and provides backpressure through `TxQueueFull`. After every two queued transmissions, the worker opens an RX window of up to 500 ms before continuing; S-band skips this because it is TX-only. The queue does not implement priority, preemption, emergency bypass, retries, persistence, or duplicate suppression. `Radio::start_transmit` is private, so there is no public `Radio` direct-send path. The low-level `Lr1121` driver commands are public but are not a supported bypass because they skip worker pacing, RX suspension/resumption, and TX state tracking. A future priority or emergency path must be an explicit API that still uses the radio worker and does not skip duplicate handling.

Duplicate suppression must not be applied when enqueuing: retries intentionally reuse `packet_id`, and a retry must remain queueable after the original transmission is lost. Duplicate detection belongs on the receive/forwarding path.

### Forwarding and Duplicate Detection

The packet handler has a volatile 64-entry duplicate cache and hop-limited forwarding. It learns the origin ID of each accepted non-self, non-duplicate packet. Locally addressed direct text is delivered to the crypto handler: paired senders require authenticated decryption, unknown senders may send plaintext introductions, and paired contacts never accept plaintext fallback. Accepted text is written to chat storage and the UI is notified with a popup and a revision. The receiver queues an ACK only after the record persisted; see the ACK semantics above.

For each received packet:

1. Validate its structure and protocol version.
2. Check the duplicate cache using (from, packet_id). Drop duplicates from local chat delivery; if an identical direct-text packet was previously accepted, re-queue its ACK without adding another chat entry.
3. Process the packet locally if to matches this device’s ID or is a broadcast address.
4. If hop_limit is zero, do not forward it.
5. Otherwise decrement hop_limit and forward, without changing any other field.

The initial hop limit is 3. Packets addressed to the receiving device are processed locally and are not forwarded by default. The hop_limit is the only mutable header field and is excluded from the AEAD AAD tracking.

## 7. Persistent Storage

The firmware stores the local Device ID, typed settings, keyed contacts and indexed conversation messages using `storage::storage_manager::StorageManager`. The shared secret is never persisted. Identity regeneration is explicit and requires pairing again.

### Flash Store

`hardware::flash::FlashStore` provides persistent typed settings, contacts and indexed message records on ESP internal flash. `storage::storage_manager::StorageManager` is the application-facing facade. It wraps `esp_storage::FlashStorage` for async use and delegates page management and wear leveling to `sequential-storage`. See [Storage and identity](storage-and-identity.md) for current initialization and usage examples.

Construct the store with the ESP flash peripheral and the absolute flash address range reserved for application data:

```rust
let mut store = FlashStore::new(peripherals.FLASH, data_start..data_end)?;
```

The range must belong to a dedicated data partition, start and end on flash erase-page boundaries, and contain at least two pages. `MapConfig::try_new` checks the range geometry, but does not check the device partition table or prevent overlap with firmware. Writes can erase sectors inside the supplied range. Never use a range occupied by the bootloader, partition table, firmware, or another data owner. This repository currently uses the `storage` partition declared by `partitions.csv`; see [Storage and identity](storage-and-identity.md#startup-and-partition-requirements) for its bounds and how to verify the flashed table.

Settings use named keys and typed values. Keep the value type for a setting name consistent, because the stored record does not include a type tag. Names are limited to 31 UTF-8 bytes. Supported primitive values include booleans, signed and unsigned integers, and `f32`/`f64`.

```rust
store.set("display_brightness", &80u8, &mut buffer).await?;
let brightness = store.get::<u8>("display_brightness", &mut buffer).await?;
```

Contacts are keyed by their 16-byte device ID and saved with `write_contact`; `read_contact` retrieves one by ID. Their serialized record contains the ID, a 32-byte derived peer key, and a 32-byte custom-name field. The key is stored without application-level encryption.

Provide a work buffer large enough for the largest serialized key and value, rounded up to the flash word size. For a contact record, use at least 160 bytes. Keep a settings buffer large enough for the largest setting key and value you store.

### Discovery packet types
- Type: 0x05
- To: 0x0000...0000 (broadcast)
- Payload: empty
- Encryption: false
- Purpose: Active discovery request. It is unauthenticated and reveals the sender's public device ID; receivers may relay it and record the sender, but this firmware never answers one automatically.

#### DeviceAnnounceResponse (`0x07`)
- Type: `0x07`
- To: The requesting device ID (unicast)
- Payload: Empty
- Encryption: false
- Purpose: Return the responder's public ID in the `from` header. This proves neither identity nor possession of a pairing secret. This firmware parses the type but never sends it: a device reveals its ID by sending an introduction, message, or pairing frame to a peer the user selected.

#### PairingConfirm (`0x08`)
- Type: `0x08`
- To: The other device ID (unicast)
- Encryption flag: true; payload carries an authenticated stage byte, the fresh nonce(s) relevant to that stage, the AEAD nonce, and the tag (no plaintext secret).
- The lower device ID initiates. The responder issues a fresh challenge, and the initiator's final proof binds both nonces. These control frames are relayed without decryption; endpoints save a contact only after successful verification.

#### Discovery and unpaired introductions

Discovery is passive as well as active: accepted valid packet origins are added to the bounded nearby list, and a user can also broadcast a `DeviceAnnounce`. No automatic reply is sent, so a device is listed by the peers it has actually transmitted to; a quiet device that never announces or sends traffic stays undiscoverable. Discovery IDs are public and unauthenticated. A discovered unpaired peer can receive an explicitly warned plaintext introduction; pair with a strong shared secret before sending private information.
