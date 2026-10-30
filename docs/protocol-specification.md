# Protocol reference

This describes the bytes this firmware puts on the air and the rules it applies to them. Constants are defined in `src/bin/packet/packet_data.rs`; framing and crypto live in `src/bin/packet/` and `src/bin/crypto/mod.rs`. Radio modulation settings are separate from this protocol and are covered in [radio and packet path](packets-and-radio.md).

## Version and header

Protocol version 2 uses a fixed 47-byte header followed by a variable payload. There is no payload-length field: the frame length reported by the radio defines the payload end. All multi-byte integers are big-endian.

| Offset | Size | Field | Meaning |
| ---: | ---: | --- | --- |
| 0 | 1 | `version` | Must be `2`; other versions are rejected. |
| 1 | 4 | `packet_id` | Identifies this fragment for duplicate suppression and retry matching. |
| 5 | 4 | `message_id` | Identifies the logical message shared by all fragments. |
| 9 | 1 | `fragment_index` | Zero-based; must be less than `fragment_count`. |
| 10 | 1 | `fragment_count` | 1–16; zero is invalid. |
| 11 | 16 | `from` | Sender's public device ID. |
| 27 | 16 | `to` | Recipient device ID; all-zero means broadcast. |
| 43 | 1 | `is_encrypted` | `0` or `1`; other values are invalid at parse time. |
| 44 | 1 | `packet_type` | One of the values below. |
| 45 | 1 | `initial_hop_limit` | Hop limit chosen by the sender. |
| 46 | 1 | `hop_limit` | Remaining relays; must not exceed the initial value. |
| 47 | n | `payload` | Type-specific bytes; encrypted layout adds nonce and tag. |

The version byte is not negotiable: a board running this firmware rejects version 1 frames, so both ends of a conversation must run the same protocol version.

A single-packet message has `fragment_index = 0` and `fragment_count = 1`.

`PacketData::validate` rejects an unsupported version, zero or over-16 fragment count, index ≥ count, payload beyond 208 bytes, an encrypted payload shorter than the 28-byte GCM overhead, and a remaining hop limit above the initial value. `PacketDeserializer` additionally rejects a frame shorter than 47 bytes, longer than 255 bytes, an encryption flag other than 0/1, and an unknown packet type.

## Packet types

| Value | Variant | Current use |
| ---: | --- | --- |
| `0x01` | `Text` | Chat text; plaintext broadcast or encrypted direct. |
| `0x02` | `Ack` | Authenticated direct-message acknowledgement. |
| `0x03` | `Coordinates` | Eight-byte position payload, single fragment. |
| `0x04` | `Heading` | Declared type; no sender or handler in this firmware. |
| `0x05` | `DeviceAnnounce` | Plaintext broadcast request; empty payload. |
| `0x06` | `FrequencyChange` | Declared announcement type; no handler or automatic retune. |
| `0x07` | `DeviceAnnounceResponse` | Plaintext direct add-contact request; sent from the devices screen. It pairs nothing and only prompts the peer. |
| `0x08` | `PairingConfirm` | Authenticated three-stage pairing frame. |

Unknown type bytes never reach the packet handler because deserialization fails.

## Device IDs and local packet IDs

A device ID is 16 random bytes created from the ESP32-S3 TRNG and stored in flash. It is a public address, not a secret and not cryptographic proof of identity. Regenerating it changes the key-derivation context, so peers must pair again.

The `packet_id` and `message_id` fields are 32-bit. Locally generated packets take IDs from one process-wide allocator whose counter is stored in flash under the `packet_ids` setting. A fragmented chat message reserves one contiguous block: the first value is the message ID and each fragment increments `packet_id` by its index. Pairing, ACK, and announcement sends reserve one ID each from the same allocator.

Every reservation writes the advanced counter to flash before any of its IDs can reach the air, then hands out the range. A reboot therefore resumes at the first unused ID: an ID that was sent is never replayed, and only a reservation interrupted before it was queued is skipped. Holding the flash session for the whole reserve-and-write keeps concurrent producers from claiming the same range. The counter wraps after 4,294,967,296 reservations. The settings screen's PACKET IDS page shows the last assigned packet and message ID, the next ID, and the percentage of the 32-bit space left.

A `debug-console` build (described below) can reset the counter to zero over the cable for test setups.

`message_id` is used to correlate a whole logical message and to identify an ACK target. It is not an encryption nonce: AES-GCM has its own random 12-byte nonce per encrypted packet.

## Fragmentation

Text payloads are limited to 512 bytes (`MAX_LOGICAL_MESSAGE_SIZE`) and 16 fragments. The sender computes the fragment count from the active band's maximum encoded size, then splits the plaintext into equally sized chunks except for the final short one.

| Band profile | Maximum encoded packet | Plaintext bytes per fragment | Encrypted plaintext bytes per fragment |
| --- | ---: | ---: | ---: |
| Sub-GHz LoRa | 255 | 208 | 180 |
| 2.4 GHz LoRa | 255 | 208 | 180 |
| S-band LR-FHSS | 133 | 86 | 58 |

The values are `max_packet_size - 47` for plaintext and `max_packet_size - 47 - 28` for encrypted payloads. A 512-byte message therefore needs at most three fragments on either LoRa profile, nine encrypted fragments on S-band, or six plaintext fragments on S-band. Messages exceeding the logical limit or the band-sized fragment budget are rejected before queueing.

Every fragment of one logical text message shares `message_id`, sender, recipient, packet type, and encryption state. Each fragment has a distinct `packet_id` and index. Each encrypted fragment is encrypted separately after its header is final, so its tag authenticates the fragment index/count and other immutable header fields.

The receiver authenticates each direct fragment before adding plaintext to an assembly. It accepts at most 16 fragments, 512 assembled bytes, and four incomplete assemblies (oldest evicted). Assemblies are keyed by sender, destination, and message ID and require consistent count, type, encryption state, and initial hop limit. Fragments are joined in index order. Missing fragments produce no delivered message and no ACK; the sender retries the whole message at the application level.

Relays forward fragments independently without decrypting them, so a relay never needs the pair key.

## Payload encoding

### Plaintext (`is_encrypted = 0`)

`payload` is exactly the application bytes. For text this is the fragment's slice of UTF-8 text; for coordinates it is the eight-byte position; for `DeviceAnnounce` it is empty.

### Encrypted (`is_encrypted = 1`)

```text
payload = nonce (12 bytes) || ciphertext (same length as plaintext) || tag (16 bytes)
```

AES-256-GCM uses the 32-byte pair key. The 46 header bytes through `initial_hop_limit` are the associated data. `hop_limit` is excluded so a relay can decrement it without invalidating the tag. The sender takes the nonce from the ESP hardware random source; the receiver reads the nonce and tag from the payload.

A receiver with a saved contact key rejects a packet that does not authenticate, and also rejects plaintext from a paired contact. Encrypted traffic from a sender with no saved key is rejected. A plaintext packet from an unknown sender is accepted as an unpaired introduction. There is no plaintext fallback after a failed decryption.

The AES-GCM layer hides and authenticates payload contents and authenticates header fields, but it does not hide sender/recipient device IDs, packet type, encryption flag, fragment metadata, hop limits, or timing.

### Text

After reassembly the plaintext must be valid UTF-8 and within the storage limit (512 bytes). It is stored once as a chat record. Invalid text is dropped and not acknowledged.

### Coordinates (`0x03`)

Exactly eight bytes:

| Bytes | Encoding |
| --- | --- |
| 0–3 | Signed 32-bit big-endian latitude in microdegrees, −90,000,000…90,000,000 |
| 4–7 | Signed 32-bit big-endian longitude in microdegrees, −180,000,000…180,000,000 |

Coordinates must arrive as a single fragment. Broadcast coordinates must be plaintext. Direct coordinates follow the contact-key rules. Out-of-range or wrong-length payloads are discarded.

### Acknowledgement (`0x02`)

The ACK application payload is exactly 36 bytes:

| Bytes | Meaning |
| --- | --- |
| 0–15 | Original sender's device ID |
| 16–31 | Original recipient's device ID |
| 32–35 | Acknowledged logical `message_id`, big-endian |

The ACK header identifies the acknowledging device as `from`, the original sender as `to`, repeats the logical `message_id`, and sets `is_encrypted = 1`. The payload is AES-GCM protected with the same pair key and the ACK header as AAD. The receiver checks that the payload names this device as the original recipient and the ACK sender as the original sender, so a valid tag alone is not enough to mark an unrelated message.

An ACK is generated only for an encrypted direct message that has been fully validated and successfully persisted, or found among the conversation's newest stored records. Broadcast, plaintext introductions, and unauthenticated coordinates are never acknowledged. An ACK reports receiver-side storage acceptance, not display or reading.

The sender retransmits an unacknowledged direct message after 20 s, up to three transmission rounds. A retry reuses the original packet IDs, fragment metadata, nonces, and ciphertext, so it matches the receiver's duplicate cache and acceptance cache: the receiver repeats the ACK instead of storing a second row. An authenticated ACK cancels the retry. Broadcast and plaintext sends are not retried.

The sender matches an ACK to its own records by conversation plus the 32-bit logical message ID. The stored record keeps the full widened ID, so an ACK cannot match an older record that happened to use the same low 16 bits.

### Device announcement (`0x05`)

`DeviceAnnounce` is a plaintext broadcast with an empty payload. It announces the sender's public ID in the header. It is not authenticated, it proves no pairing secret, and this firmware never replies automatically with `DeviceAnnounceResponse`. Receipt adds the sender to the bounded nearby list. The contacts screen's ADD DEVICE action is what queues this packet.

`DeviceAnnounceResponse` is the add-contact request: a plaintext direct frame with an empty payload, sent when the user picks ASK TO ADD. Receipt shows an "add me" popup naming the sender and stores nothing; a request from a blocked peer is dropped. It carries no key and starts no handshake.

## Pairing

Both users enter the same secret locally; it is never put on the wire. Each device derives:

```text
key = PBKDF2-HMAC-SHA256(
    password = entered secret bytes,
    salt     = "communicator-pair-v2" || lower_device_id || higher_device_id,
    iterations = 100_000,
    dkLen = 32)
```

Sorting the two public IDs makes both sides derive the same key regardless of who initiated. The UI accepts any non-empty secret up to 64 characters, but password strength still determines resistance to offline guessing; a one-character secret is trivially guessable despite the KDF. The secret itself is not persisted. The derived contact key is stored in flash without application-level encryption.

The lower device ID initiates, and stages are carried in a single `PairingConfirm` payload byte:

| Stage | Value | Payload bytes (after the stage byte) |
| --- | ---: | --- |
| Request | 0 | AEAD nonce (12) + tag (16) — 29 bytes total |
| Challenge | 1 | request nonce (12) + AEAD nonce (12) + tag (16) — 41 total |
| Finish | 2 | request nonce (12) + response nonce (12) + AEAD nonce (12) + tag (16) — 53 total |

Each frame has an empty plaintext, so the ciphertext length is zero. Its associated data is the 46-byte immutable header plus the stage byte, plus the earlier exchanged nonces that the stage echoes: none for `Request`, the request nonce for `Challenge`, and both nonces for `Finish`. The frame's own AEAD nonce is transmitted in the payload and is not part of the AAD. The exchanged nonces are fresh per handshake, and the final proof binds both. Verifying the tag proves the sender knows the derived key over that stage context. A captured old frame does not satisfy a new exchange because the nonce context differs.

Flow:

1. Lower ID sends `Request`.
2. Higher ID verifies it, stores the request nonce, and sends `Challenge` with a new response nonce.
3. Lower ID verifies the challenge against the stored request nonce, stores the response nonce, and sends `Finish`.
4. Higher ID verifies `Finish` against both nonces, persists the contact, and reports success.
5. Lower ID persists its contact after verifying the challenge and sending its finish frame.

The pending handshake is a single slot in `PairingCoordinator`; a new attempt replaces it. Frames are unicast and require the destination ID to match. A failed verification ends the attempt without saving a contact. After identity reset, the ID changes and pairing must be repeated.

Public device IDs are exchanged in headers throughout discovery and pairing. Pairing does not provide forward secrecy: the derived key is static for the ID pair and secret, and flash stores it as key bytes. Physical access to the device and its flash contents is inside the threat model.

## Forwarding and hop limits

The default hop limit is 3. A relay decrements `hop_limit` and queues the same packet only if it is a broadcast or not addressed to itself. A packet with `hop_limit = 0` is not forwarded. Relayed copies keep all fields except the mutable hop byte. Duplicate suppression occurs at the receiving node before local handling and before forwarding.

Because this is a simple flood, multiple relays may transmit duplicates of the same frame. The receiver's 128-entry seen cache and the reassembler's duplicate detection bound repeated processing, but the protocol has no route discovery, acknowledgements for broadcasts, congestion control, or persistent forwarding queue.

## What is not implemented

- No per-fragment retransmission; the outbox retries a whole message after 20 s, up to three rounds.
- No radio-level acknowledgement or over-the-air frequency negotiation. The worker enforces a 10 % transmit duty cycle per band.
- No handling for `Heading` or `FrequencyChange`.
- No group/broadcast encryption.
- No persistent replay protection. Duplicate suppression is a 128-entry RAM seen cache for received packets, plus a check of the newest eight stored records of that conversation before a record is appended; that second check is what suppresses a retransmit after a reboot. Packet-ID allocation itself is persistent: the counter advances in flash before any of its IDs can be sent.
- No compression or binary text encoding; chat text is UTF-8 bytes.