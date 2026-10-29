# Chat history, fragmentation, and navigation

This guide covers the chat-specific behavior on top of the message lifecycle: how history is paged, what the row indicators mean, how text is fragmented, and how a received coordinate can be used for simple navigation. The send/receive path itself is in [message lifecycle](messaging.md); wire sizing is in [protocol reference](protocol-specification.md).

## History storage and paging

Every conversation has its own indexed record sequence in flash (see [storage and identity](storage-and-identity.md)). The chat view shows four records at a time (`MAX_CHAT_MESSAGES = 4`).

- Opening a conversation loads its newest page.
- Up/Down move the selection while the composer is empty and the send menu is closed.
- Up on the oldest visible row requests the previous page if one exists; the record immediately before that row becomes selected, so browsing continues without skipping records (the loaded page may overlap the previous one).
- Down on the newest visible row requests the following page; the first row of that newer page becomes selected.
- Down past the newest record clears the selection.
- Typed draft text or an open send menu suppresses history browsing, so Up/Down only navigate history when the composer is empty.

The UI keeps only the visible page in RAM. Older records are read from flash by ordinal when requested, so long histories do not grow the UI memory footprint. A new-message notification triggers a reload of the newest page, which can move a reader who was browsing older records back to the newest page; there is no automatic “stay where I was reading” behavior.

`ROW_TEXT_CHARS` limits each drawn row to 28 characters, and a preview line is shorter still. Text is truncated for display only; the stored record keeps the full body up to 512 bytes. A row also records whether it was sent by this device, whether it was encrypted, its acknowledged flag, and optional coordinates.

## Row indicators

| Situation | Display |
| --- | --- |
| Outgoing text | `ME> text` |
| Received direct text | `FROM text` |
| Received broadcast text | `<sender label> > text` |
| Received coordinate | `GPS<- lat,lon` |
| Sent coordinate | `GPS-> lat,lon` |

Broadcast sender labels come from the local contact book: the saved contact name, or `UNHEARD xx..` (first two ID bytes) for a sender with no saved contact. The label is resolved locally from the record's sender ID; it is not carried in the message payload, so two devices can display different labels for the same broadcast.

Color and status:

- An outgoing encrypted direct row is muted and shows `SENDING..` until an authenticated ACK is stored, then is drawn green.
- A received encrypted row is cyan.
- Plaintext broadcast and unpaired-introduction rows are muted and never show delivery status.

The ACK indicator reflects a stored, authenticated ACK from the peer: the peer validated and persisted the complete message. It does not mean the recipient read or displayed it. The flag is stored in the sent record, so it survives a reboot. If no ACK arrives, the outbox retransmits the same fragments after 20 s, up to three transmission rounds. After the last round the outbox drops the message and the row keeps `SENDING..`; the failure is logged but not shown on screen.

## Ack rules

An ACK exists only for an encrypted direct message:

- Broadcast traffic has no shared key and is never acknowledged.
- Plaintext introductions and unauthenticated coordinates are never acknowledged.
- Text is acknowledged only after every fragment has been authenticated, reassembled, UTF-8-validated, and persisted.
- Direct coordinates are acknowledged after payload validation and persistence.
- The ACK payload names the original sender, recipient, and logical message ID and is authenticated with the pair key, so a forged or misrouted ACK cannot mark a record.

A duplicate of an already accepted encrypted direct fragment can produce another ACK without producing another chat row, which covers a lost ACK for a repeated fragment. A fresh send of the same text is a new message with new IDs and is stored again.

## Text sizing and fragmentation

The protocol header is 47 bytes. Each band caps the total encoded packet; the fragmenter subtracts the header and, for encrypted packets, the 28-byte GCM overhead.

| Band | Encoded limit | Plaintext per fragment | Encrypted text per fragment |
| --- | ---: | ---: | ---: |
| Sub-GHz LoRa | 255 | 208 | 180 |
| 2.4 GHz LoRa | 255 | 208 | 180 |
| S-band LR-FHSS | 133 | 86 | 58 |

Limits are 512 bytes of logical text and 16 fragments. So a maximum-size encrypted message takes three fragments on either LoRa band and nine on S-band; the same text plaintext takes six fragments on S-band. Send fails before queueing when the body or fragment count exceeds the limit for the active band.

Each fragment gets its own `packet_id` and its own AES-GCM nonce/tag; all fragments share `message_id`, sender, recipient, type, and encryption state. The tag covers the immutable header (including fragment index and count) but not the mutable hop limit, so relays can decrement hops without breaking authentication.

Reassembly is bounded: four incomplete assemblies in RAM, 16 fragments per message, 512 bytes assembled, oldest assembly evicted when the cache is full. There is no time-based expiry of a partial assembly and no per-fragment retransmission request. A missing fragment means that attempt is never delivered or acknowledged, and the sender's retry resends the whole message.

Queueing is not atomic. If the TX queue fills mid-message, earlier fragments are already committed to the queue and may transmit while later ones never do; the receiver then cannot assemble the message. The sender's local save happens only if queueing succeeded for every fragment, so a partial queue attempt leaves no sent record and the draft text in place.

## Coordinates

A coordinate message is a single fragment with an eight-byte payload: signed big-endian latitude and longitude in microdegrees. The receiver rejects wrong lengths and values outside ±90° latitude or ±180° longitude. Coordinates must not be fragmented, and broadcast coordinates must be plaintext; a broadcast marked encrypted is rejected.

A received coordinate appears as a selectable `GPS` row in that conversation. Pressing Enter with that row selected opens the navigation screen. Sending a position uses the same encryption rules as text in that conversation: encrypted to a paired contact, plaintext on broadcast or an explicitly unencrypted introduction.

## Navigation screen behavior

The navigation screen compares the received target with the latest local GPS fix:

- The target coordinate is always displayed.
- With no local fix, it reports that it is waiting for a fix.
- With a fix, it computes great-circle distance (haversine, mean Earth radius 6,371,000 m) and the absolute target bearing clockwise from north, and shows the distance in metres plus a compass abbreviation and bearing in degrees.
- Within 5 metres it reports arrival.
- Otherwise, if a movement-based course estimate exists, it shows a turn correction: straight within ±15°, else left or right, plus the device's current movement direction.
- If there is no course estimate yet, it asks the user to walk, because course is derived from movement.

Course estimation uses successive local fixes: the first valid fix anchors the estimate, and once the device has moved at least five metres the bearing between anchor and current fix becomes the course and the anchor advances. GPS drift at rest can therefore produce a noisy or unavailable course, and a stationary device cannot produce one at all.

Limitations to keep in mind:

- There is no compass or heading sensor; “turn left/right” compares movement direction to the target bearing, not the direction the device is facing.
- There is no map, road network, route calculation, turn-by-turn guidance, or safety-critical accuracy claim.
- Cached GPS values have no age indicator on this screen; a stale fix looks like a current one.
- The target is the peer's transmitted position at send time, which may be outdated or inaccurate.
- An authenticated encrypted coordinate binds the sender and payload to the saved pair key, but says nothing about the accuracy or freshness of the GPS measurement. An unencrypted coordinate has no sender authentication at all.