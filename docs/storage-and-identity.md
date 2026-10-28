# Storage and identity

## Storage policy

`storage::storage_manager::StorageManager` is the application-facing API. It uses the dedicated internal-flash partition for settings, device identity, contacts and message history. There is no SD-card backend or runtime storage-location setting.

Chat history has exactly two writers, both calling `add_messages` directly:

- the receive path (`packet_handler.rs`) after decrypting and reassembling an accepted message, and
- the UI send path (`ui/mod.rs`) right after the fragments are accepted by the TX queue.

The UI never caches messages across screens: it reads the records it needs per event. See [Message delivery](messaging.md) for the notifications that coordinate this.

`storage::session::Flash` is the shared handle to the device and its scratch buffer. `Flash::with(async |session| ...)` runs one storage call with exclusive access and returns before anything else needs the lock; RX, the UI, and the flush task all go through it, so no caller has to manage the guard.

| Data | Backend | API |
| --- | --- | --- |
| Typed settings | Internal flash | `set_setting` / `get_setting` |
| Device identity | Internal flash | `load_device_id` / `save_device_id` / `regenerate_device_id` |
| Contacts and pairwise keys | Internal flash | `save_contact` / `load_contact` |
| Direct and broadcast history | Internal flash | Append and paged-read methods |

Message history is append-only: append one logical chat message when it is created, and do not append it again for each radio fragment or retry. Reads return bounded pages, so callers can process long conversations without holding all records in RAM. Continue using the returned `next_offset`; start at zero to read a conversation from its beginning.

## Startup and partition requirements

`main.rs` calls `esp_hal::init`, starts the runtime, then uses project-specific helpers to initialize flash storage, the TRNG, display, radio and GPS. The storage setup constructs `FlashStore` over the reserved partition and passes it to `StorageManager::from_flash`. On first boot it creates a device identity if one is absent.

The board readback previously reported 4 MiB flash with `nvs` at `0x9000`, `phy_init` at `0xf000`, and a factory app beginning at `0x10000`; it did not contain a data partition. `partitions.csv` defines a 2 MiB factory app and a `storage` partition from `0x210000` to the exclusive end `0x400000`. See the [ESP-IDF partition table guide](https://docs.espressif.com/projects/esp-idf/en/stable/esp32s3/api-guides/partition-tables.html).

To inspect the table currently flashed on a connected board:

```sh
espflash read-flash 0x8000 0x1000 /tmp/communicator-partition-table.bin --chip esp32s3
espflash partition-table /tmp/communicator-partition-table.bin --to-csv
```

`0x8000` is the standard ESP-IDF table offset; use the configured offset if the bootloader was changed. The `.cargo/config.toml` runner passes this repository's `partitions.csv` when flashing. Verify the board reports `storage,data,undefined,0x210000,0x1f0000`. Back up data before installing a changed layout because flashing can change app and storage partition boundaries.

`main.rs` sets `FLASH_DATA_START = 0x210000` and `FLASH_DATA_END = 0x400000`, matching the `storage` row. The end is exclusive. Keep both addresses erase-page aligned. `MapConfig::try_new` validates range geometry but cannot validate the partition table or prevent overlap; writes may erase pages inside the supplied range. Never point these constants into NVS or outside the declared storage partition.

## Initialization example

The custom initialization in `main.rs` follows this pattern after `esp_hal::init` and runtime startup:

```rust
use crate::{
    hardware::flash::FlashStore,
    storage::storage_manager::StorageManager,
};

// Use the bounds of the dedicated, erase-page-aligned storage partition.
let flash = FlashStore::new(
    peripherals.FLASH,
    FLASH_DATA_START..FLASH_DATA_END,
)?;
let mut storage = StorageManager::from_flash(flash);
let mut flash_buffer = [0u8; 512];
```

The TRNG entropy source must remain alive while generating an identity:

```rust
let _trng_source = esp_hal::rng::TrngSource::new(peripherals.RNG, peripherals.ADC1);
let trng = esp_hal::rng::Trng::try_new()?;

if storage.load_device_id(&mut flash_buffer).await?.is_none() {
    storage
        .regenerate_device_id(&mut flash_buffer, |id| {
            trng.read(id);
            Ok(())
        })
        .await?;
}
```

## Settings and identity

Setting names must be nonempty and at most 31 UTF-8 bytes. A setting's value type must remain stable across firmware versions because stored values have no type tag. Missing settings return `None`. The display backlight uses the `display_brightness` key with a `u8` value from 0 to 100; it defaults to 80 and is applied during startup.

```rust
storage.set_setting("display_brightness", &80u8, &mut flash_buffer).await?;
let brightness: Option<u8> = storage
    .get_setting("display_brightness", &mut flash_buffer)
    .await?;
```

Identity is stored in flash as exactly 16 bytes. Create one only when absent; regenerate it only for an explicit identity reset. Regeneration does not update contacts, pairings or a running radio. The manager retries if the generator repeatedly returns the existing ID and does not save a replacement in that case.

## Contacts

`Contact` stores a 16-byte device ID, a `KEY_SIZE` derived key and a custom UTF-8 name with capacity 32 bytes. The key is the lookup key for the flash map. `display_name()` returns the custom name or the ID as 32 uppercase hexadecimal characters. With the current 32-byte AES key, its fixed record is 80 bytes. `main.rs` uses a 512-byte reusable flash scratch buffer. This leaves room for the largest current message plus map key and storage overhead; increase it if message or record sizes grow.

Peers that message this device without a saved contact do **not** get a flash record: they exist only as runtime rows in the UI's contact book until a real pairing creates one, and their block state is therefore session-only.

Contact keys are serialized without application-level encryption. Paired direct messages use AES-256-GCM in the packet path, but the contact key itself is not protected at the application serialization layer; see [Security status](#security-status).

## Saving and loading messages

`FlashStore` stores messages as indexed map entries, with a per-conversation record count. Direct messages use the peer device ID as the conversation key; broadcast history uses `[0xff; 16]`. This storage key is only an internal conversation identifier. A broadcast packet still uses its normal all-zero radio destination.

### Save a locally sent text

Build one `StoredMessage` for the logical chat message, set `from_me` to `true`, and append it under the recipient's device ID. In a fragmented message, use the logical `message_id` for `StoredMessage.id`, and the packet ID for `StoredMessage.packet_id`. Call this from the real application send path alongside packet creation and queueing. Save once per logical message, not once per radio fragment or retransmission. The current firmware has no application clock, so the caller must choose a timestamp source or a documented placeholder until one is added.

```rust
use heapless::String;
use crate::storage::message::{MessageKind, StoredMessage, MAX_MESSAGE_PAYLOAD};

let mut text = String::<MAX_MESSAGE_PAYLOAD>::new();
text.push_str(input_text).map_err(|_| "message is too long")?;

let stored = StoredMessage::new(
    u32::from(message_id),
    Direction::Sent,
    0, // timestamp seconds; no wall clock is available yet
    MessageKind::Text,
    text,
    first.packet_id,
    first.message_id,
    device_id,
    encrypted,
);

storage
    .append_messages(&conversation.storage_id(), &[stored], &mut flash_buffer)
    .await?;
```

For a sent broadcast, `Conversation::storage_id()` maps `broadcast` to
`[0xff; 16]`; the radio packet's broadcast destination remains `[0; 16]`.

The UI persists a sent message after every fragment is accepted into the TX queue. Queue acceptance is not proof of over-the-air delivery. The receiver sends an ACK after an accepted direct text or coordinate message is persisted, but this does not prove that the message was displayed. When the sender receives an ACK for an encrypted direct message, the matching sent record's `acknowledged` flag is rewritten in flash, and the row's grey "SENDING.." becomes a green "OK"; plaintext and broadcast messages are not marked and never change once sent.

### Save a received message

After RX processing has validated and, for a paired direct message, authenticated/decrypted a message addressed to this device, build a `StoredMessage` with `from_me: false`. Use the sender's device ID as the direct conversation key. Store a received broadcast with `append_broadcast_messages`. Do not persist relayed packets as chat messages; forwarding and local chat delivery are separate operations.

The current receive path persists accepted direct text and coordinates directly: text is decrypted per fragment, reassembled up to 512 bytes, then UTF-8-validated before a record is appended. A direct-message ACK is queued only after all required authentication, payload validation, and persistence succeed. Do not store encrypted bytes as display text; decode/decrypt first.

Deleting history is part of the API: `delete_conversation(device_id)` removes a peer's records and `delete_all_conversations()` clears every conversation, both used by the contact-manage and chats screens. Deletions bump the storage revision, so list previews update without a full redraw.

### Read conversation history

Start at offset zero when opening a conversation. Render the returned records, then request another page with its `next_offset` if `done` is false. For a paginated UI, retain the cursor or reload earlier pages when the user scrolls back. The cursor is an index into that conversation, not a global message ID.

The UI keeps it simple and reads only recent history:

```rust
// The four most recent records of one conversation, oldest first.
let records = storage
    .load_recent_messages(&conversation.storage_id(), 4, &mut flash_buffer)
    .await?;
```

Paged reads still exist for older history:

For broadcast history, use `load_broadcast_messages_page(offset, count, buffer)`. The page size limits the number returned in one call. The largest current message is a coordinate message with a 200-byte text payload (221 serialized bytes); the 512-byte scratch buffer accommodates one record and map overhead. Returned `page.messages` is a RAM vector containing up to the requested page size, so keep that size modest.

There is currently no history limit, pruning or compaction policy in `StorageManager`. The `storage` partition is finite; append calls eventually return an error if it cannot accept more records. Choose a retention policy and show storage-full errors in the UI before relying on long-running message history.

## Security status

- Paired direct text, coordinate, and ACK payloads use AES-256-GCM with immutable packet-header context as AAD; unpaired introductions, unpaired coordinates, and their ACKs may be plaintext and unauthenticated.
- Pair keys are derived with PBKDF2-HMAC-SHA256 from the shared secret and both sorted public device IDs. Device IDs alone are not secret key material.
- Contact records hold derived keys in plaintext at the application serialization layer.
- A received-message ACK exists only for an encrypted direct message. Its payload names the original sender, recipient, and logical message ID and is authenticated with the contact key. It indicates acceptance by the receiver's storage, not display. Only a matching encrypted direct-message row gets the green check, and the flag is persisted with the message record.
- Text fragmentation/reassembly is bounded to 16 packets and 512 application bytes; incomplete assemblies are volatile and may be evicted. Received GPS navigation relies on local GPS fixes and is not a compass or road-routing system. Discovery IDs remain unauthenticated, and relays forward protected packets without decrypting them.
