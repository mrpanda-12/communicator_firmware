# Chat, message delivery, fragmentation, and GPS navigation

## Chat message and ACK indicator

The chat reads its rows from the flash-backed message store: every accepted
message is written to the `storage` partition immediately — receive-path
messages by the packet handler, sent messages by the UI send path. Opening a
conversation loads its four most recent records. A sent message is shown as
soon as all of its fragments are accepted by the transmit queue; this records
a local send attempt, not over-the-air delivery.

An ACK exists only for an **encrypted direct** message: broadcast traffic and plaintext introductions are never acknowledged. The ACK payload is 34 bytes and names the acknowledged message — the original sender's 16-byte device ID, the original recipient's 16-byte device ID, and the logical message ID as two big-endian bytes — and the ACK header repeats that message ID. The sender matches the message ID together with the peer's device ID. A green check is shown only on a matching **encrypted direct-message** row after an ACK has been received.

The whole ACK payload is protected by AES-256-GCM using the saved pair key and the ACK's immutable packet header as associated data. A receiver rejects an ACK it cannot authenticate, and rejects one whose payload does not name this device as the original recipient, so an unauthenticated ACK cannot mark a row. The check means that the recipient accepted and persisted the complete message. It does not prove that the text was rendered or read.

Unlike the volatile indicator of earlier revisions, the acknowledged flag is **written to the sent message's flash record**, so delivery marks survive a reboot. The sender does not automatically retry transmissions.
The receiver sends ACKs for accepted encrypted direct text and coordinate messages. It waits until a text message is fully reassembled, decoded as UTF-8, and persisted before acknowledging it. A duplicate retry of a previously accepted fragment may cause another ACK, but does not create another chat row.

## Text packet sizing and fragmentation

The packet header is 43 bytes. Each transmission must fit both the protocol's 255-byte maximum and the active radio band's smaller maximum, if any. The fragmenter chooses an application-data chunk size after reserving header space and, for encrypted packets, the AES-GCM nonce/tag.

| Radio profile | Encoded packet limit | Plaintext bytes per fragment | Encrypted text bytes per fragment |
| --- | ---: | ---: | ---: |
| Sub-GHz LoRa | 255 | 212 | 184 |
| 2.4 GHz LoRa | 255 | 212 | 184 |
| S-band LR-FHSS | 133 | 90 | 62 |

The current chat accepts at most 512 bytes of logical text and at most 16 fragments. Therefore a 512-byte message needs three fragments on either LoRa profile, nine encrypted fragments on S-band, and six plaintext fragments on S-band. Messages larger than the logical limit are rejected before queueing.

Each fragment has a different `packet_id`; all fragments belonging to one text message share the same `message_id`, sender, destination, and packet type. Encrypted fragments are encrypted independently after their index and count are set. Each fragment therefore has its own nonce and authentication tag, and its tag covers the immutable packet header, including fragment metadata. The mutable remaining-hop field is excluded from authentication so relays can forward the packet.

The receiver validates and authenticates each encrypted fragment before storing its plaintext. It reassembles fragments in index order and checks consistent sender, destination, message ID, fragment count, packet type, encryption state, and original hop limit. Only a complete message within 512 bytes is converted to UTF-8 and sent to chat. A missing, malformed, conflicting, unauthenticated, or invalid-UTF-8 fragment set does not produce a chat entry or ACK.

Reassembly holds four incomplete messages in RAM; when full, the oldest assembly is evicted. There is no time-based expiry, retransmission protocol, or persistent fragment state. The radio queue holds 16 packets, but a multi-fragment enqueue is not atomic: if the queue fills partway through a message, already queued fragments cannot be rolled back, and the receiver will not display that incomplete message. Relays forward fragments independently and do not decrypt them.

## Received GPS coordinates

`Coordinates` packets use an eight-byte application payload:

| Bytes | Value | Encoding |
| --- | --- | --- |
| `0..4` | Latitude | Signed 32-bit integer, big-endian, in microdegrees |
| `4..8` | Longitude | Signed 32-bit integer, big-endian, in microdegrees |

The receiver accepts only latitude from -90,000,000 through 90,000,000 and longitude from -180,000,000 through 180,000,000. It discards payloads of any other length or outside those ranges. A paired sender must encrypt the coordinate packet with its saved contact key; an unknown, unpaired sender may send an unencrypted coordinate, which is public and unauthenticated. Coordinate data is not currently appended to persistent chat history.

A valid coordinate appears as a `GPS` row in the conversation that received it. The row participates in the chat's Up/Down selection. With the composer empty, press Enter while that row is selected to open its navigation screen; press Back to return to the conversation. The device can also transmit its own position from the chat composer's send menu, using the same encryption rules as a text message.

## Navigation behavior and limitations

The navigation screen compares the received target with the latest local GPS fix. It calculates great-circle distance and an absolute target bearing (clockwise from north). It also estimates the device's course from successive local fixes. Once the fixes show at least five metres of movement, the display compares that course with the target bearing and reports a left, right, or straight correction.

The GPS driver provides position fixes but no compass heading or course API. Course is consequently an estimate from movement, not the device's facing direction: remain still and the UI asks the user to walk to establish direction. GPS drift can create a noisy course, especially over short distances, and cached GPS fixes have no explicit age indicator. The display is not a map, road-routing service, or safety-critical navigation system; verify the location and direction visually before relying on them.

The coordinate is the peer's transmitted location, not a claim that the peer is still there. An authenticated encrypted coordinate binds the sender and packet contents to the saved pair key, but does not make the GPS measurement accurate or fresh. An unencrypted coordinate has no sender authentication.
