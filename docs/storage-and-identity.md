# Storage and Identity

## Integration Status

`main.rs` does not construct a `FlashStore`, load or regenerate the device identity, or persist contacts yet. The storage and identity APIs exist and compile, but no application startup path calls them. Packet encryption and authenticated storage remain unimplemented, as described under [Security Status](#security-status).

## Flash Store

`storage::flash::FlashStore` wraps ESP flash with `sequential-storage` map storage and an async adapter. Construct it with an explicitly reserved flash range:

```rust
let mut store = FlashStore::new(peripherals.FLASH, data_start..data_end)?;
```

The range must be a dedicated application-data partition, aligned to erase-page boundaries, and contain at least two pages. `MapConfig::try_new` checks range geometry but does not inspect the partition table or prevent overlap with firmware. Writes may erase pages in the range. The project has no safe default flash range; derive the addresses from the actual partition layout before enabling writes.

Use a scratch buffer large enough for the serialized key and value. A contact record is 80 bytes; allow at least 128 bytes for its key/value and alignment overhead. Size settings buffers for the largest setting stored.

## Typed Settings

Settings use nonempty string names up to 31 UTF-8 bytes. Keep the type associated with each name stable; the stored item does not add a type tag.

```rust
store.set("display_brightness", &80u8, &mut buffer).await?;
let brightness: Option<u8> = store.get("display_brightness", &mut buffer).await?;
```

`get` returns `None` when no value exists. The map supports the primitive value types provided by `sequential-storage`'s `Value` implementations.

## Contacts

`Contact` contains a 16-byte `device_id`, a derived 32-byte AES key, and a custom name with capacity 32. `write_contact` stores by device ID; `read_contact` retrieves by that ID. If no custom name is set, `display_name` returns the ID as uppercase hexadecimal. The fixed record is 80 bytes. The derived key is stored without application-level encryption; protect the flash partition accordingly.

## Device Identity

`device::identity::load_device_id` reads the `device_id` setting and returns `Option<[u8; 16]>`. `regenerate_device_id` reads the current value, fills a new ID from ESP-HAL `Trng`, avoids returning the exact same ID, writes the replacement, and returns it.

Only call regeneration for an explicit identity reset. The TRNG must have a supported entropy source active while reading. Regeneration does not update contacts, re-pair peers, or alter an active `Radio`; `main.rs` does not currently initialize or invoke this service.

## Security Status

- `crypto/mod.rs` provides software AES-256-GCM `derive_key`, `encrypt`, and `decrypt` free functions plus SHA-256 helpers (`hash`, `hash_with_salt`). Packet encryption and authenticated packet handling are not integrated yet.
- `derive_key` currently hashes the password with SHA-256 plus a fixed domain separator. It is not HKDF and does not bind device IDs.
- Contact records store the derived 32-byte key, not the password. The key is still plaintext at the application serialization layer.
- Storing only a password hash would not preserve the key needed to decrypt messages; the derived key must be stored or re-derived from a re-entered password.
- The protocol specification describes intended identity/key behavior; it should not be interpreted as implemented security until crypto and authenticated packet handling are added.