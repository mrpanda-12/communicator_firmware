# Message lifecycle

This guide follows one logical chat message through the code. The UI state machine lives in `src/bin/ui/state.rs`; command execution, fragmentation, sending, and flash-backed history live in `src/bin/ui/mod.rs`; radio framing is in `src/bin/packet/`; receive dispatch is in `src/bin/network/packet_handler.rs`; and the shared flash session is in `src/bin/storage/session.rs`.

## Conversations and stored records

The UI and network use `ConversationId`:

- `Broadcast` represents radio destination `[0; 16]` and is stored under `[0xff; 16]` so it cannot collide with a peer ID.
- `Direct(peer_id)` represents one peer and is stored under that peer's 16-byte device ID.

Each complete logical chat entry is one `StoredMessage`. A message may consist of several radio packets, but fragments are not stored as separate chat rows. The storage map assigns each conversation a monotonically increasing 32-bit record ordinal and stores a separate record count.

Incoming records have `from_me = false`, sender ID, packet/message IDs, encryption state, type, and text or coordinates. Outgoing records have `from_me = true`. Current callers supply timestamp `0`; it is a placeholder, not GPS time. An encrypted direct outgoing record starts unacknowledged. A later authenticated ACK updates its acknowledged flag.

## Outbound path

### From keypress to send request

The keypad worker debounces a key and sends one `KeypadKey` through the static event channel. The UI loop passes it to `UiState::handle_keypad`. In a chat, two hex digits compose one ASCII character. ENTER opens the send menu; the selected action creates either a text `OutgoingMessage` or a coordinate message from the latest valid GPS fix. The pending command is consumed by the UI loop.

### Validation, encryption, IDs, and fragmentation

`send_message` refuses to continue if the local device identity is unavailable. For a direct conversation, it obtains the key from the in-memory contact book. A normal paired direct send requires that key; the explicitly unencrypted introduction path is separate. Broadcast text has no group key and is plaintext.

Before building packets, the sender determines whether this body is encrypted, selects the radio band's maximum encoded packet length, and computes the number of fragments. It reserves exactly that many IDs from the shared local packet-ID allocator, then `Fragmenter` assigns `base_id + fragment_index` to each packet and uses the base ID as the shared `message_id`. IDs are 32-bit wire values; the counter wraps only after 4,294,967,296 reservations. Each reservation writes the advanced `packet_ids` value to flash before handing the IDs out, so a reboot resumes at the first unused ID and never repeats one. All local producers use the same allocator, so locally generated packets do not use separate unsynchronized counters.

For each encrypted direct fragment, `build_message_packets` finalizes the fragment header before AES-256-GCM encryption. The ciphertext payload contains a 12-byte nonce, ciphertext, and 16-byte tag. The tag authenticates the immutable header; relays can decrement the remaining-hop field without invalidating it. Failed key lookup, invalid packet sizing, or encryption failure aborts before enqueueing.

### Queueing, retransmission, and local flash save

The UI builds every fragment and submits the whole message to the two-slot outbox. Submission is all-or-nothing: a third waiting message is refused with an on-screen alert and the draft stays in the composer. An outbox task drains the slots every 200 ms into the radio's 32-entry FIFO. Fragments the FIFO refuses stay in their slot and are retried on the next pass, so a temporary full queue delays a message instead of dropping part of it.

After one full transmission round the outbox waits 20 s and retransmits the same encoded fragments, up to three rounds in total. An authenticated ACK cancels the slot immediately. Retries reuse the original packet IDs and ciphertext, so a receiver drops them through its duplicate cache and repeats its ACK instead of storing a second row. Broadcast and plaintext sends have no ACK and are not retried. The outbox lives in RAM; a reboot ends pending retries.

Once the message is in the outbox, the UI builds one sent `StoredMessage` and appends it to flash under the conversation's storage key. A successful append updates the row preview, bumps the history revision, and clears the composer. If the append fails, an alert is shown and the draft remains, but the outbox keeps transmitting. Queue acceptance means the worker accepted encoded frames, not that the peer received them.

The radio worker owns actual transmission. Before each frame it takes airtime from the band's 10 % duty-cycle budget and waits when the budget is empty, then sends the frame and waits for TX IRQ completion. An encrypted direct sender shows “SENDING..” until the receiver's ACK is authenticated and saved. Broadcast/plaintext rows have no ACK state.

## Inbound path

### Frame-level work

The radio worker receives one encoded packet and calls `PacketHandler::handle_packet`. The handler:

1. Deserializes and validates version, lengths, flags, fragments, and hop-limit fields.
2. Reads the current local ID from the `DEVICE_ID` watch and drops traffic if identity is missing or the sender ID is this device's own ID.
3. Classifies broadcast/unicast and checks its bounded `(sender ID, packet_id)` duplicate cache.
4. Remembers accepted origins for the nearby-device list.
5. Dispatches supported packet types to their local handler. Unconsumed broadcasts and packets for another device may be relayed if the remaining hop limit allows it.

The duplicate cache is RAM-only. A repeated packet key is dropped from normal processing; for a matching already accepted encrypted direct fragment, the handler may queue its ACK again. Because duplicate IDs are recorded before local handling, an invalid packet with a fresh ID is not retried through the normal path by repeating that same ID during this session.

### Direct authentication

For direct text, coordinates, and ACKs, `decrypt_direct` looks up the sender contact in flash. A blocked contact is rejected. A saved contact must use the encrypted form and pass AES-GCM authentication; plaintext from a paired contact is rejected. An unknown sender may send plaintext introductions, but encrypted traffic without a saved key is rejected. Thus plaintext acceptance is an explicit unpaired path, not a fallback after decryption failure.

Broadcast text is delivered only when unencrypted. Encrypted broadcast has no shared key and is not displayed locally. Broadcast coordinates must also be plaintext. Packet forwarding is distinct from chat delivery: a relayed packet is not written to a conversation just because the relay received it.

### Reassembly and validation

Text fragments are authenticated/decrypted individually when needed and added to `Defragmenter`. A set is identified by sender, destination, and message ID and is limited to 16 fragments and 512 plaintext bytes. Up to four incomplete assemblies are held in RAM; the oldest assembly is evicted when a fifth begins. Conflicting fragment metadata or duplicate fragment-index content discards the assembly. Fragments are joined by index, not arrival order. A complete text body must be valid UTF-8 and no larger than the stored-message limit.

Coordinates are single-fragment messages. Their payload must be exactly eight bytes: signed big-endian latitude and longitude in microdegrees. Values outside the WGS84 numeric ranges are rejected.

### Persist, notify, and acknowledge

Before appending, the handler compares the delivery against the newest eight records of that conversation. A retransmit that arrives after a reboot therefore matches its stored record, produces no second row, and still gets its ACK. A new delivery is appended once. Only after the append succeeds does the receive handler queue a popup and increment the history revision. If the append fails, an alert is shown, no popup is sent, and no ACK is generated.

An encrypted direct text ACK is queued only after full reassembly, UTF-8 validation, and persistence. An authenticated direct coordinate is acknowledged only after its coordinate payload is validated and stored. Plaintext and broadcast traffic is never acknowledged. ACK payloads name the original sender, recipient, and message ID; the ACK is encrypted using the pair key and authenticated header. On reception, the ACK must authenticate and name the expected exchange before storage marks the matching sent record acknowledged.

An ACK means the peer accepted and persisted the message; it does not prove the message was displayed or read. If the ACK is lost, the outbox retransmits the message after 20 s, so the peer's duplicate handling repeats the ACK. A later exact duplicate of an accepted encrypted direct fragment may trigger another ACK while the acceptance cache entry remains.

## Flash I/O and endurance implications

Normal message history uses append-style map entries. One outgoing chat record is appended after all fragments are queued; one incoming chat record is appended after a complete message is accepted. The firmware does not append a row per radio fragment. Reading a conversation or paging it does not write flash. A sent encrypted message can receive a later ACK update, which stores an updated message value after scanning records for the matching sent ID; this is a separate write and can cost more than the original append.

Settings and contacts are replaced through the map API. Identity reset writes one new identity. The RX path does read a peer contact record for direct packets; that is a flash read, not a flash write. Repeated storage failures are surfaced in logs, but most message-send failures are not currently presented in a dedicated on-screen error state.

`Flash::with` protects the map and its shared 640-byte work buffer with one mutex. Keep the operation scoped; do not hold the guard across unrelated work. See [storage and identity](storage-and-identity.md) for partition layout and record encoding.

## Opening and paging chat history

At boot, the UI reads the latest record for each visible conversation to seed list previews. Opening a conversation calls `load_message_page` for its newest four records, oldest first. While the composer is empty, Up/Down selects neighboring records. Reaching the edge requests the preceding/following page; the UI reads at most four records by ordinal and updates the displayed window. Older messages remain on flash rather than accumulating in RAM. Enter on a selected coordinate row opens navigation; a normal selected text row remains selected.

History change notifications reload the newest page when the active conversation changes or when the UI receives a new revision. Therefore a new message may move the view back to the newest page. Page navigation itself is a read-only flash operation.

## Operational failures and limits

| Failure | Observable effect |
| --- | --- |
| No local device ID | Outbound chat/announcement is refused; RX handler drops traffic it cannot address. |
| No direct pair key on an encrypted send | Send is refused before packet construction. |
| GCM authentication failure or missing key | Direct packet is rejected; no row or ACK. |
| Missing/malformed/conflicting fragment | No complete text record; no text ACK. Reassembly RAM is bounded. |
| Invalid UTF-8 or oversized message | No stored chat row and no ACK. |
| TX queue full | The outbox keeps unsent fragments and retries them every 200 ms. |
| Outbox full | The third waiting message is refused with an alert; the draft stays in the composer. |
| No ACK after three transmissions | The outbox drops the retry and the row keeps its unacknowledged state; the failure is logged. |
| Conversation at 128 records | The oldest record is deleted before the new one is appended. |
| Flash write failure after queueing | Packets may still transmit, but no sent row is confirmed locally and the composer remains. |
| Flash write failure on receive | No popup and no ACK. |
| Lost ACK | The outbox retransmits after 20 s, up to three rounds; after that the row stays unacknowledged. |
| History page read failure | The page loader currently falls back to an empty page; this is logged only indirectly and is a UI limitation. |
| Storage partition full | Appends fail and the UI shows a `STORAGE FULL` alert; each conversation already drops its oldest record at 128. |

The receiver's duplicate and reassembly state is volatile. Reboot clears it, so a retransmit is recognized only by the stored-record check over the conversation's newest eight rows. The peer's IDs and ciphertext remain visible at the packet-header level; encryption protects payload contents and integrity, not traffic metadata.