# Storage and identity

All persistent state lives on the ESP32-S3's internal flash inside the `storage` partition. There is no SD-card backend and no runtime choice of storage location. `hardware/flash.rs` provides the typed map; `storage/storage_manager.rs` is the application-facing facade; `storage/session.rs` serializes access from the UI and receive path.

## Partition and startup

`partitions.csv` declares:

```text
nvs,data,nvs,0x9000,0x6000
phy_init,data,phy,0xf000,0x1000
factory,app,factory,0x10000,0x200000
storage,data,undefined,0x210000,0x1f0000
```

`main.rs` opens the absolute range `0x210000..0x400000` (end exclusive), which matches that row. `MapConfig::try_new` validates the range geometry only: it cannot verify the flashed partition table or prevent overlap, and normal operation can erase 4 KiB pages inside the range. Never point these constants at NVS, app, or bootloader space.

Before first use, confirm what the board actually has:

```sh
espflash read-flash 0x8000 0x1000 /tmp/communicator-partition-table.bin --chip esp32s3
espflash partition-table /tmp/communicator-partition-table.bin --to-csv
```

0x8000 is the standard ESP-IDF partition-table offset; if the bootloader was changed, use the configured offset. The `.cargo/config.toml` runner passes this repository's `partitions.csv` when flashing.

Startup opens the map, then:

1. Reads the device ID, or generates one if absent.
2. Reads display brightness and UTC offset settings, writing defaults when missing.
3. Loads saved contacts into the in-memory contact book, skipping any record whose ID equals this device.
4. Moves the storage manager plus its 640-byte scratch buffer into the shared `Flash` mutex.

If the storage range constants are invalid, `fatal` logs `Boot stopped: ...`, waits ten seconds, and reboots without touching flash.

## Access pattern

`Flash::with` locks one `FlashSession` containing the `StorageManager` and its buffer, runs one async operation, and releases the lock when the closure returns:

```rust
let saved = flash.with(async |session| {
    session.storage
        .append_messages(&conversation.storage_id(), &[stored], &mut session.buffer)
        .await
        .is_ok()
}).await;
```

Both the receive path and the UI use this handle, so they cannot run overlapping map operations or share the scratch buffer accidentally. Do the storage work inside the closure and do not hold the guard across unrelated awaits.

`FLASH_BUFFER_SIZE` is 640 bytes. The largest current record is a 512-byte text message, which serializes to 546 bytes: 33 fixed bytes, 512 text bytes, and one acknowledged flag. Raise `FLASH_BUFFER_SIZE` if record types or limits grow; `sequential-storage` also needs room for key and page metadata in the same work buffer.

## Key namespaces

`StorageKey` prefixes every key with a one-byte namespace so settings, contacts, and message data cannot alias:

| Namespace | Key contents | Used for |
| --- | --- | --- |
| `s` | setting name bytes (≤31 UTF-8 bytes) | Typed settings. |
| `c` | 16-byte device ID | One contact record. |
| `m` | 16-byte conversation ID + 4-byte little-endian record ordinal | One stored message. |
| `n` | 16-byte conversation ID | Next record ordinal (count) for that conversation. |
| `b` | 16-byte conversation ID | Oldest retained record ordinal (base) for that conversation. |

A key is serialized as its length byte followed by the namespace and value bytes, matching `sequential-storage`'s key encoding.

`ConversationId::storage_id()` maps broadcast to `[0xff; 16]` so it cannot collide with a real peer ID; radio destinations still use `[0; 16]` for broadcast. Direct conversations are keyed by the peer's device ID.

## Typed settings

Settings are named keys with values serialized without a type tag, so a setting's Rust type must stay stable across firmware versions and a name must be non-empty and at most 31 bytes. Missing settings read as `None`; a type mismatch or invalid stored value is treated as a read failure by callers.

| Setting | Type | Default | Loaded behavior |
| --- | --- | --- | --- |
| `display_brightness` | `u8` | 80 | Values above 100 are rejected and the default is used. Applied to GPIO7 PWM at boot; no runtime adjustment screen exists. |
| `timezone_offset_minutes` | `i32` | 0 | Clamped range −720…840 minutes (UTC−12:00…UTC+14:00). Out-of-range values fall back to 0. Changed from the GPS settings screen and written as it changes. |
| `packet_ids` | two `u32` little-endian values: packet ID and message ID | 0 | Next packet ID and the base ID of the last reservation. Loaded at boot, then rewritten by every reservation before the IDs are used, so a reboot resumes at the first unused ID. |

Identity is stored under the setting key `device_id` as exactly 16 bytes.

## Device identity

The identity is 16 random bytes from the ESP32-S3 TRNG. It is public: it appears in packet headers. It is used as a destination address, as part of the pairing salt, and as the sender key for conversation records.

- First boot generates one if none exists.
- `regenerate_device_id` keeps generating until it differs from the current value, up to eight attempts, then saves the new value. Repeated TRNG output equal to the current ID is reported as an error.
- Settings → Device → Reset identity triggers regeneration. The new ID is published through the `DEVICE_ID` watch, so the UI and receive path use it without a reboot.
- Contacts and chat history remain on the device across an identity reset. They are keyed by peer IDs, not by the local ID. Peers must pair again because the pairing salt changes.

If the entropy source or flash is unavailable, identity-dependent operations are refused and the affected feature reports failure.

## Contacts

A `Contact` stores the peer's 16-byte device ID, the 32-byte derived pair key, a 32-byte custom-name field, and a blocked flag. Its serialized size is 81 bytes.

- A contact is created from the devices screen by picking a peer and entering a name plus an optional password. Only a non-empty password runs the pairing handshake and saves a pair key; an empty password adds the peer with no key, so its traffic stays plaintext. A peer that is only heard on air stays in the in-memory discovery lists until the user adds it.
- `display_name()` returns the custom name if set, otherwise the 32-character uppercase hex ID. Contact management sets, changes, and removes that name, and renaming a saved contact writes the record back to flash; every list and chat label reads the name through this one function.
- A blocked contact is dropped on the receive path before any message is processed and is hidden from the lists.
- Blocking or deleting a contact is written through the contact record and the removal path; an unpaired/unknown peer's block state exists only for the running session.
- Contact records, including the pair key, are stored as plain map values. The firmware does not encrypt its flash.

## Message records

`StoredMessage` is the chat-history unit. It contains:

| Field | Encoding |
| --- | --- |
| `id` | 4-byte little-endian conversation ordinal (also the record's index when written) |
| `from_me` | 1 byte, 0/1 |
| `timestamp` | 4 bytes little-endian UTC seconds since 2000-01-01; 0 without a dated GPS fix |
| `packet_id` | 4 bytes little-endian |
| `message_id` | 4 bytes little-endian |
| `sender_id` | 16 bytes |
| `encrypted` | 1 byte, 0/1 |
| kind tag | 1 byte: `0x01` text, `0x02` coordinates |
| coordinates | 8 bytes little-endian (two `i32` microdegree values) for coordinate records only |
| text length | 2 bytes little-endian |
| text | UTF-8 bytes, up to 512 |
| acknowledged | 1 byte, 0/1 |

The record stores both packet and message IDs in their full 32-bit wire width. ACK matching compares the stored `message_id` with the acknowledged ID in the ACK payload.

Only one record is written per logical chat message. The send path appends after all fragments are accepted by the TX queue; the receive path appends after full validation, decryption, and reassembly. Fragments and relays are never stored as chat rows.

`append_messages` reads the conversation count and base, writes each message at `message(id, count)` while incrementing, and writes the updated count last. Writing the count last means a partial failure leaves the count unchanged, so a retry reuses the same indices instead of creating gaps.

A conversation keeps at most 128 records. Before appending when the count minus the base reaches that cap, `append_messages` deletes the record at the base and advances the base entry, so the oldest row leaves as the newest arrives. The base key is the oldest retained ordinal; paging clamps a requested start to it.

## Reading and paging

`load_message_page(device_id, start, max_messages, buffer)` returns a `MessagePage { total, base, start, messages }`:

- With `start = None` it returns the newest `max_messages` records.
- With `Some(n)` it returns records from ordinal `n`, clipped to the retained range `base..total`.

The UI displays four records at a time and requests adjacent pages as the user scrolls. Records are returned oldest-first within a page, and the base value stops paging below the oldest retained record. `load_recent_messages` remains a thin wrapper over the same call for preview reads. Reads do not write flash.

`delete_conversation` removes the message records and count for one conversation. `delete_all_conversations` enumerates every `m`/`n` key and deletes them in bounded batches so the RAM it uses does not grow with history size. These operations remove chat records; they do not erase contacts, settings, or identity.

Retention removes the oldest record of one conversation when its 128-record cap is reached; there is no global pruning or compaction. The partition is finite, `append_messages` reports a storage error when the map cannot accept a write, and the UI shows a `STORAGE FULL` alert for a failed append.

## Acknowledgement updates

When a valid ACK arrives for an encrypted direct message, `mark_acknowledged` scans that conversation's records and rewrites each matching, unacknowledged record sent by this device, comparing the full 32-bit logical message ID. Only `from_me` records are eligible, so an ACK cannot mark a received row that shares a message ID. This is the only path that rewrites an existing message record; it costs a read scan plus one write per matching record.

## Write characteristics

- Chat history is append-only in normal use, which suits a flash log: page rewrites happen through `sequential-storage` page management rather than per-packet rewrites.
- The RX path reads a contact record for each direct payload it handles. That is a read, not a write, but it means direct traffic performs a flash lookup per fragment.
- Settings written mid-session (for example, each UTC-offset step) commit immediately.
- Power loss during a write can leave a partially updated map entry; `sequential-storage` handles page-level recovery, but the firmware does not keep a separate write-ahead journal or a second copy of chat history.
- Chat writes are throttled by design (one record per logical message). There is no logging-to-flash per packet or per fragment, which avoids a wear problem, but there is also no automatic compaction beyond the storage library's page management.

## Security notes for storage

- Device IDs and conversation keys are not secret; the pairing key is.
- Contact keys are stored as plain bytes.
- The entered pairing secret is not stored anywhere, including logs.
- Chat text and coordinates are written as plaintext record fields even when they arrived encrypted over the air. The packet layer's encryption does not extend to records, so anyone with physical access to the flash can read them.
- Erasing a conversation removes its records through the storage map; it does not guarantee physical erasure of every cell copy on the flash device.

See [messaging](messaging.md) for the end-to-end write/read flow and failure behavior, and [protocol reference](protocol-specification.md) for over-the-air key derivation and framing.