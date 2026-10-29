# Receiving a radio packet

`PacketHandler::handle_packet` in `src/bin/network/packet_handler.rs` is called by the radio worker after it reads one encoded frame. It owns a bounded duplicate cache, a bounded accepted-fragment cache, and a fragment reassembler. It does not own the radio or UI; it receives references to the TX queue, discovery tracker, pairing coordinator, and shared flash session.

## One frame from IRQ to handler

The radio worker starts LoRa receive when the active profile supports RX. On an RX IRQ it reads the reported payload length and bytes from the LR1121, clears the IRQ, restarts RX, then awaits the packet callback. While TX is active, RX is not active except for the configured short listen window after a burst. Header/CRC errors are cleared and RX is restarted without calling the packet handler.

The handler then applies these checks in order:

1. `PacketDeserializer::deserialize` reads the fixed 47-byte header and remaining payload. A short or malformed frame is rejected.
2. `PacketData::validate` checks protocol version, fragment index/count, payload bounds, encryption overhead, and hop limits. Invalid frames are rejected before dispatch.
3. The current local identity is read from the `DEVICE_ID` watch. If absent, the frame is ignored; after identity reset, future packets use the newly published ID.
4. A packet whose sender ID equals the local ID is discarded as an echo.
5. The handler classifies the destination and checks its `(sender, packet_id)` duplicate cache. A duplicate is dropped before discovery, delivery, or forwarding; an accepted encrypted direct fragment may trigger an ACK repeat if its fingerprint and message context match the acceptance cache.
6. The sender is added to the bounded nearby-device list, and one frame-level RX log is emitted.
7. Supported local packet types are dispatched. Unconsumed broadcasts and packets for a different destination may be relayed after decrementing `hop_limit`.

`PacketData::is_for` includes broadcast packets. Consequently a broadcast is considered “here” for the log's address classification even though it is not a direct unicast.

## Dispatch table

| Packet | Local condition | Action | Follow-up |
| --- | --- | --- | --- |
| `PairingConfirm` | Unicast addressed to this ID | Verify and advance one handshake stage | Consume; may queue challenge/finish and persist contact on completion. |
| `Ack` | Unicast addressed to this ID | Decrypt, validate exchange IDs, mark sent record acknowledged | Consume. |
| `Text` | Broadcast, plaintext | Reassemble and save under broadcast conversation | Relay broadcast if hop remains. |
| `Text` | Unicast addressed here | Enforce key rules, decrypt if paired, reassemble, validate UTF-8, save | Consume; authenticated direct messages are ACKed only after successful save. |
| `Coordinates` | Broadcast or addressed here | Require one fragment, decrypt direct if paired, validate 8-byte coordinate, save | Broadcast can be relayed; authenticated direct coordinates may be ACKed after save. |
| Other types / unmatched traffic | — | No local chat action | Broadcast or packet addressed elsewhere may be relayed. |

Encrypted broadcast text is not delivered locally. The protocol has no shared broadcast key. A direct packet addressed here that fails authentication is not decrypted with a plaintext fallback. The code drops unknown encrypted senders, missing keys, bad GCM tags, plaintext from a saved contact, blocked contacts, invalid coordinates, and malformed text.

## Direct-key rules

`decrypt_direct` loads the contact record by sender ID from flash for each handled direct payload. It applies these cases:

| Saved contact | Packet encrypted? | Result |
| --- | --- | --- |
| Blocked | Either | Reject before decryption/delivery. |
| Saved and allowed | Yes, valid GCM tag | Return plaintext and the authenticating pair key. |
| Saved and allowed | No | Reject; there is no downgrade path. |
| No saved contact | Yes | Reject; no key is available. |
| No saved contact | No | Accept plaintext as an unpaired introduction. It is public and unauthenticated. |

AES-GCM covers packet immutable header fields as AAD; `hop_limit` is excluded because a relay decrements it. The 12-byte nonce and 16-byte tag are carried in the packet payload. IDs, routing fields, packet type, fragment shape, and timing remain visible.

## Text reassembly and storage

For direct text, each fragment is decrypted before `Defragmenter::push`. Broadcast text is supplied as plaintext. The reassembler keys an assembly by sender, destination, and message ID; it enforces matching fragment count, encryption state, type, and initial hop limit. Up to 16 fragments and 512 application bytes are accepted, with four incomplete assemblies retained in RAM. Fragments may arrive in any order; output is assembled by fragment index. A conflicting duplicate or inconsistent metadata discards the assembly. There is no fragment timeout or automatic resend.

After completion, `deliver_text` requires valid UTF-8 and a body within storage capacity. It creates exactly one `StoredMessage` with `from_me = false`, then calls `save_message`. `save_message` returns success only when the record was appended. Only a successful append calls `notify::receive`, which queues a toast and increments the history revision. No chat row or toast is fabricated from a failed write.

Broadcast history is stored under the reserved `[0xff; 16]` storage key; direct history is stored under the sender's ID. Relayed frames are not chat records on the relay.

## Coordinates

Coordinates are not fragmented by the receiver: `fragment_count` must be one. Broadcast coordinates must be unencrypted. A direct coordinate packet follows the contact-key rules. Its plaintext must contain exactly two signed big-endian 32-bit values, latitude and longitude in microdegrees; valid ranges are ±90° latitude and ±180° longitude.

The handler creates one coordinate record, appends it, and only on successful append notifies the UI. It generates an ACK only if the packet was authenticated with a saved pair key. A public unpaired coordinate is stored without authentication or ACK.

## ACKs and duplicate replies

The ACK payload is 36 bytes: original sender ID (16), original recipient ID (16), and logical message ID (4, big-endian). The ACK header sender is the acknowledging device, its destination is the original sender, and it repeats the logical message ID. The payload is AES-256-GCM protected with the pair key and ACK header AAD.

`accept_ack` requires the contact key and a valid GCM tag. It then checks that the payload names the ACK recipient as the original sender and the ACK sender as the original recipient. A valid ACK calls `mark_acknowledged` for the conversation and message ID; only sent records are modified. That operation scans stored records and rewrites matching, unacknowledged sent messages. The UI is notified to reload. This reports receiver-side persistence, not display or reading.

The handler records accepted encrypted direct fragments in a 128-entry FIFO cache with sender, packet ID, message ID, packet fingerprint, and key. A duplicate must match all these values before `repeat_ack` emits another ACK. The fingerprint covers authenticated header bytes and payload, excluding mutable hop limit. Seen IDs and accepted ACK records are volatile; a reboot clears them, and FIFO eviction eventually treats an old packet ID as new.

A delivery that survives a reboot is caught again at the storage layer: before appending, the handler compares sender, packet ID, and message ID against the newest eight records of the conversation. A match returns `SaveResult::Duplicate`, which skips the append and the popup but still allows the ACK, so a sender's retry stops without creating a second chat row.

Queueing an ACK can fail when the TX FIFO is full. Received ACKs cancel the matching outbox slot by message ID, so the sender stops retransmitting. A retry must preserve the original packet ID and encoded encrypted fragment to match the duplicate record; generating a fresh nonce/ciphertext under the same packet ID changes the fingerprint and would store a second row.

## Pairing dispatch

The pairing coordinator permits one pending handshake. The devices derive the same pair key locally from a shared secret and sorted public IDs. The lower ID starts with `Request`; the higher ID authenticates it and responds with `Challenge`; the lower ID verifies the challenge and sends `Finish`; the higher ID verifies the final proof. Request/response nonces bind the exchange. The endpoint handling the final proof persists the `Contact` and publishes a success event; the other endpoint saves the contact when it verifies the corresponding stage. Failures terminate the pending operation and report to the UI.

Pairing frame construction and verification live in `network/pairing.rs`; stage dispatch and contact persistence occur in `advance_pairing`. See [protocol reference](protocol-specification.md#pairing).

## Relaying

After local dispatch, broadcast packets and packets not addressed to this device can be passed to `forward`. The handler stops if `hop_limit` is zero; otherwise it decrements that field and submits the same packet to the radio TX queue. The mutable hop byte is not in the GCM AAD, so relaying does not require a decryption key. The queue revalidates and serializes the packet. Queue-full or validation errors are logged; no alternate send route exists.

Packets addressed to this device are consumed locally and are not relayed. This is a simple bounded flood, not a route-discovery or congestion-control protocol.

## Resource and persistence bounds

| Resource | Limit | Lifetime |
| --- | ---: | --- |
| Seen `(sender, packet_id)` keys | 128 | PacketHandler RAM; FIFO eviction. |
| Recent records checked for a duplicate delivery | 8 | Conversation flash; oldest of the window ignored. |
| Accepted encrypted fragment ACK records | 128 | PacketHandler RAM; FIFO eviction. |
| Incomplete text assemblies | 4 | PacketHandler RAM; oldest evicted when full. |
| Fragments per message | 16 | Wire validation/reassembly. |
| Reassembled application body | 512 bytes | Packet layer and storage model. |
| TX queue | 32 encoded packets | Shared queue RAM; lost at reset. |
| Message records | One per accepted logical message, at most 128 per conversation | Internal flash; oldest record deleted at the cap. |

No packet-body plaintext is logged by this module. Logs report frame metadata, errors, and state transitions. The contact key is passed into crypto and the acceptance cache, not printed. Pairing secrets and nonce contents are not logged.
