# Encryption at rest

This page covers how the flash contents are protected. The firmware stores data
in the dedicated `storage` partition (`0x210000..0x400000`): the device ID,
contact records including the 32-byte pair keys, chat records, display settings,
and the device identity. See [storage and identity](storage-and-identity.md) for
the record layouts.

## Why the firmware does not encrypt records itself

Application-level encryption needs a key the device can read at boot. If that key
sits in the same flash as the ciphertext, an attacker who reads the flash reads
the key too, so the encryption adds no protection against the threat it is meant
to stop. A meaningful key has to live outside the readable flash, which on the
ESP32-S3 means an eFuse block that hardware can use and software cannot read
back.

That is exactly what the chip's flash-encryption engine provides: an XTS-AES key
in eFuse, read-protected, used transparently by the ROM bootloader for every
flash read. The firmware therefore keeps the record formats simple and relies on
the hardware layer. No application code changes when flash encryption is on.

## What it protects and what it does not

Protects:

- Offline dumps of the flash chip, including `espflash read-flash`, a desoldered
  chip, or a backup image: contents are XTS-AES ciphertext.
- Contact pair keys and chat history when the device is powered off.

Does not protect:

- A running device: the CPU decrypts on read, so a debugger attached to running
  firmware, or a compromised firmware image, can read the data.
- RAM, the UART/USB console, or the screen.
- A peer that already holds the pair key. Message contents are end-to-end
  encrypted on air, but the peer can decrypt its own traffic.
- Metadata on air: device IDs, packet sizes, and timing stay visible.
- Forward secrecy: compromising a stored pair key still decrypts ciphertext the
  attacker recorded earlier.

## Enabling flash encryption on the ESP32-S3

The steps below follow the Espressif flash-encryption flow. They burn eFuses and
cannot be undone; a wrong key or a lost key makes the device unusable. Read the
ESP-IDF "Secure Boot and Flash Encryption" guide for your chip revision before
running any of them, and work on a spare board first.

1. Back up the current flash and the partition table.
2. Generate a key: `espsecure.py generate_flash_encryption_key flash_key.bin`.
   Keep the file offline; the eFuse copy cannot be read back.
3. Burn the key into a free key block, for example
   `espefuse.py --port /dev/ttyUSB0 burn_key BLOCK_KEY0 flash_key.bin XTS_AES_128_KEY`
   for the 128-bit variant, or burn two blocks with `XTS_AES_256_KEY_1` and
   `XTS_AES_256_KEY_2` for the 256-bit variant. Use the purpose names printed by
   `espefuse.py summary` for your chip.4. Confirm the security state with `espefuse.py summary`: the encryption-control field (`SPI_BOOT_CRYPT_CNT` on the ESP32-S3) must end up with an odd number of bits set.
5. Encrypt every image that will be written, at the address it occupies, for
   example
   `espsecure.py encrypt_flash_data --keyfile flash_key.bin --address 0x10000 --output app.enc build/communicator`.
   The second-stage bootloader must be encrypted the same way, because the ROM
   decrypts whatever it loads.
6. Write the encrypted images with `espflash write-bin <address> <file>.enc`.
   espflash 4.6 does not encrypt images itself, so a plain `espflash flash` after
   the key is burned would write plaintext that the bootloader cannot read.

Because the whole flash is decrypted on read, the `storage` partition, the
application, and the partition table are all covered. There is no per-record key
to rotate; changing the key means re-encrypting the entire image set.

## Verifying the setup- `espefuse.py summary` shows `SPI_BOOT_CRYPT_CNT` with an odd bit count and the key purpose set on the burned block.
- `espflash read-flash 0x210000 0x1000 dump.bin` returns bytes that do not decode
  as a `sequential-storage` map; the same range with encryption off starts with
  recognizable keys and record data.
- The device still boots, loads its identity and contacts, and shows chat
  history: the bootloader decrypts successfully.

## Operating rules

- Keep `flash_key.bin` offline and out of the repository.
- Do not ship a board with flash encryption partially configured.
- Replacing the application later means encrypting the new image before writing
  it; keep the key available for field updates.
