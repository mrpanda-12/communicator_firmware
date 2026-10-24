# Storage and identity

## Storage policy

`storage::storage_manager::StorageManager` is the application-facing storage API. It owns the flash and SD backends and applies a fixed routing policy:

| Data | Backend | API |
| --- | --- | --- |
| Typed settings | Internal flash | `set_setting` / `get_setting` |
| Device identity | Internal flash | `load_device_id` / `save_device_id` / `regenerate_device_id` |
| Contacts and pairwise keys | Internal flash | `save_contact` / `load_contact` |
| Direct and broadcast message history | Selected backend | Append and paged read methods |

`message_storage_location` is a typed setting stored on internal flash. Select `MessageStorageLocation::InternalFlash` or `MessageStorageLocation::SdCard` with `set_message_storage_location`; when absent, it defaults to SD. This setting selects where new reads and writes go. Changing it does not migrate existing messages, so existing history remains on its original backend. There is no fallback if the selected backend is unavailable.

Message history is append-only: append only newly created messages, never resend the full in-memory history. Reads are paged by backend cursor and maximum record count, so the caller can process long conversations without loading all records into RAM. Continue a page sequence using that backend's returned `next_offset`, and restart at zero after changing the selected backend. There is no application-level 50-message history limit.

## Startup and partition requirements

`main.rs` keeps the HAL/runtime initialization in `main` and puts project-specific setup in `init_sd_storage`, `init_flash_storage`, `init_trng`, and `initialize_storage`. Those helpers construct both low-level backends, pass them to `StorageManager::from_backends`, then load defaults and create a first-boot identity.

The attached table read from the board reports 4 MiB flash with `nvs` at `0x9000`, `phy_init` at `0xf000`, and a factory app beginning at `0x10000`. It had no data partition. `partitions.csv` now defines a 2 MiB factory app and a dedicated `storage` partition from `0x210000` through the exclusive end `0x400000`. This leaves ample room for the measured application image (about 350 KiB) and reserves the remainder for storage. ESP-IDF documents CSV format and alignment in its [partition table guide](https://docs.espressif.com/projects/esp-idf/en/stable/esp32s3/api-guides/partition-tables.html).

To inspect the table currently flashed on a connected board, dump the ESP32-S3 partition-table sector and convert it to CSV:

```sh
espflash read-flash 0x8000 0x1000 /tmp/communicator-partition-table.bin --chip esp32s3
espflash partition-table /tmp/communicator-partition-table.bin --to-csv
```

`0x8000` is the standard ESP-IDF partition-table offset; use the offset configured by your bootloader if it was changed. The table occupies one 4 KiB sector. The `espflash partition-table` command prints each partition's offset and size. The `.cargo/config.toml` runner explicitly passes this repository's `partitions.csv`, so `cargo run` flashes the same layout the firmware expects.

To install this layout, run `cargo run --bin communicator` (or use the configured `espflash flash` command) and allow it to update the partition table and firmware. This changes the app partition size; back up anything important before flashing. Verify the board now reports `storage,data,undefined,0x210000,0x1f0000` using the readback commands above. Do not point `FLASH_DATA_*` at NVS or at bytes outside the declared storage partition.

`main.rs` uses `FLASH_DATA_START = 0x210000` and `FLASH_DATA_END = 0x400000`, matching that storage row. The end is exclusive. Keep both boundaries erase-page aligned if the table changes. `MapConfig::try_new` checks range geometry only; it cannot check the partition table or prevent overlap. Flash writes may erase pages inside the supplied range.

The SD wiring in `main.rs` follows the Waveshare board pinout: GPIO13=SCK, GPIO14=MOSI, GPIO15=MISO, GPIO6=CS. The card uses SPI3 independently of the radio SPI2 bus and starts at 400 kHz. `FixedTimeSource` currently supplies a constant FAT timestamp until an RTC or GPS-backed source is connected.

The flash implementation lives in `hardware::flash`. The custom startup helpers in `main.rs` currently perform the equivalent of this initialization after calling `esp_hal::init` and starting the runtime:

```rust
use crate::hardware::{flash::FlashStore, sdcard::{FixedTimeSource, SdCardStorage}};
use crate::storage::storage_manager::StorageManager;
use embedded_hal_bus::spi::ExclusiveDevice;

// These are the current main.rs GPIO/SPI assignments for the Waveshare board.
let sd_spi_bus = Spi::new(
    peripherals.SPI3,
    Config::default()
        .with_frequency(Rate::from_khz(400))
        .with_mode(Mode::_0),
)
    .expect("initialize SD SPI3")
    .with_sck(peripherals.GPIO13)
    .with_mosi(peripherals.GPIO14)
    .with_miso(peripherals.GPIO15);
let sd_cs = Output::new(peripherals.GPIO6, Level::High, OutputConfig::default());
let sd_spi_device = ExclusiveDevice::new(sd_spi_bus, sd_cs, embassy_time::Delay)
    .expect("create SD SPI device");
let sd = SdCardStorage::new(sd_spi_device, embassy_time::Delay, FixedTimeSource);

// FLASH_DATA_START/END must come from the real partition table.
let flash = FlashStore::new(peripherals.FLASH, FLASH_DATA_START..FLASH_DATA_END)
    .expect("invalid flash data partition range");
let mut storage = StorageManager::from_backends(flash, sd);
let mut flash_buffer = [0u8; 256];
```

The current `main.rs` checks the flash constants before constructing either backend and stays in a loop if they are unset. This prevents the application from writing to an unknown flash range. Keep the SPI assignment aligned with the board schematic; do not copy placeholder flash addresses.

`main.rs` also keeps the TRNG entropy source alive and creates the first identity only when storage has none:

```rust
let _trng_source = esp_hal::rng::TrngSource::new(peripherals.RNG, peripherals.ADC1);
let trng = esp_hal::rng::Trng::try_new().expect("initialize TRNG");

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

Setting names must be nonempty and at most 31 UTF-8 bytes. The value type for a setting must remain stable across firmware versions because values are not stored with a type tag. Missing settings return `None`.

```rust
use crate::storage::message::MessageStorageLocation;

storage.set_setting("display_brightness", &80u8, &mut flash_buffer).await?;
let brightness: Option<u8> = storage
    .get_setting("display_brightness", &mut flash_buffer)
    .await?;

// The selection itself is stored in internal flash. When unset, it defaults to SD.
storage
    .set_message_storage_location(MessageStorageLocation::InternalFlash, &mut flash_buffer)
    .await?;
```

Identity is always stored in flash as exactly 16 bytes. On first boot, load it and generate it only if absent. Regenerate only for an explicit identity reset; doing so does not update contacts, pairings, or an active radio. Keep ESP-HAL's TRNG entropy source active while filling the ID. The manager retries if the generator returns the existing ID repeatedly, and does not save a replacement on that failure.

## Contacts

`Contact` stores a 16-byte device ID, a `KEY_SIZE` byte derived key, and a custom UTF-8 name with capacity 32 bytes. The key is the lookup key for the flash map. `display_name()` returns the custom name or the ID as 32 uppercase hexadecimal characters. With the current 32-byte AES key, its fixed record is 80 bytes. Size the reusable flash scratch buffer for the largest key/value plus map and alignment overhead; 256 bytes is used in `main.rs`.

Contact keys are serialized in flash without application-level encryption. Protect the device and partition accordingly. Packet encryption and authenticated storage are not integrated; see [Security status](#security-status).

## Message history backends

The flash backend stores each message as an indexed map entry and keeps a per-conversation record count. SD stores an append-only log. Direct-message filenames are `msgv2_` followed by the 32 uppercase hexadecimal characters of the peer ID. Broadcast history uses `msg_broadcast_v2`. Each SD record contains a 4-byte little-endian payload length followed by one serialized `StoredMessage`.

```rust
let mut message_buffer = [0u8; 256];

storage
    .append_messages(&peer_id, core::slice::from_ref(&new_message), &mut message_buffer)
    .await?;

let mut offset = 0;
loop {
    let page = storage
        .load_messages_page(&peer_id, offset, 32, &mut message_buffer)
        .await?;
    // Render or process page.messages, then drop the page before loading another.
    offset = page.next_offset;
    if page.done {
        break;
    }
}
```

The page size limits only the number returned in one call. SD uses byte offsets; flash uses record indices. The largest current message is a coordinate message with a 200-byte text payload: 221 serialized bytes, plus the 4-byte SD record header. A 256-byte message scratch buffer is sufficient for one current record. The SD reader reports truncated headers and payloads rather than silently skipping them.

The record format does not migrate existing unframed message files. The new filenames use the `v2` suffix so they do not collide with older logs. There is no recovery/commit marker for an interrupted append, so power loss can leave a truncated final record. SD removal and I/O errors are returned to the caller. `embedded-sdmmc` I/O is synchronous and can block the Embassy task while the card is busy. FAT and the `u32` file offset impose a limit below 4 GiB per file.

## Security status

- `crypto/mod.rs` has software AES-256-GCM helpers and SHA-256 helpers, but packet encryption and authenticated packet handling are not integrated.
- `derive_key` uses SHA-256 with a fixed domain separator; it is not HKDF and does not bind device IDs.
- Contact records hold derived keys in plaintext at the application serialization layer.
- The protocol specification describes intended behavior; it does not establish that encryption, authentication, forwarding, or fragmentation is implemented.
