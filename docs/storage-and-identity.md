# Storage and Identity

## Integration Status

`main.rs` does not construct a `FlashStore`, load or regenerate the device identity, or persist contacts yet. The storage and identity APIs exist and compile, but no application startup path calls them. Packet encryption and authenticated storage remain unimplemented, as described under [Security Status](#security-status).

## Flash Store

`storage::flash::FlashStore` wraps ESP flash with `sequential-storage` map storage and an async adapter. Construct it with an explicitly reserved flash range:

```rust
let mut store = FlashStore::new(peripherals.FLASH, data_start..data_end)?;
```

The range must be a dedicated application-data partition, aligned to erase-page boundaries, and contain at least two pages. `MapConfig::try_new` checks range geometry but does not inspect the partition table or prevent overlap with firmware. Writes may erase pages in the range. The project has no safe default flash range; derive the addresses from the actual partition layout before enabling writes.

Use a scratch buffer large enough for the serialized key and value. A contact record is 112 bytes; allow at least 160 bytes for its key/value and alignment overhead. Size settings buffers for the largest setting stored.

## Typed Settings

Settings use nonempty string names up to 31 UTF-8 bytes. Keep the type associated with each name stable; the stored item does not add a type tag.

```rust
store.set("display_brightness", &80u8, &mut buffer).await?;
let brightness: Option<u8> = store.get("display_brightness", &mut buffer).await?;
```

`get` returns `None` when no value exists. The map supports the primitive value types provided by `sequential-storage`'s `Value` implementations.

## Contacts

`Contact` contains a 16-byte `device_id`, a password string with capacity 64, and a custom name with capacity 32. `write_contact` stores by device ID; `read_contact` retrieves by that ID. If no custom name is set, `display_name` returns the ID as uppercase hexadecimal. The fixed record is 112 bytes. Password bytes are stored in plaintext by this serializer; application-level encryption is not present.

## Device Identity

`device::identity::load_device_id` reads the `device_id` setting and returns `Option<[u8; 16]>`. `regenerate_device_id` reads the current value, fills a new ID from ESP-HAL `Trng`, avoids returning the exact same ID, writes the replacement, and returns it.

Only call regeneration for an explicit identity reset. The TRNG must have a supported entropy source active while reading. Regeneration does not update contacts, re-pair peers, or alter an active `Radio`; `main.rs` does not currently initialize or invoke this service.

## Security Status

- `crypto/aes.rs` is empty. Flash contacts, packet encryption, AES-GCM verification, and key derivation are not implemented by this repository state.
- `crypto/hash.rs` provides SHA-256 truncation, not HKDF or a password KDF.
- Contact password storage is plaintext at the application serialization layer.
- The protocol specification describes intended identity/key behavior; it should not be interpreted as implemented security until crypto and authenticated packet handling are added.