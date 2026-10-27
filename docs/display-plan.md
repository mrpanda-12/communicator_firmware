# Display UI: Current State and Plan

## Implemented now

- The ILI9341 TFT is initialized by `hardware/display.rs` using `mipidsi` and `embedded-graphics` over hardware SPI3 at 10 MHz. UI state, contacts, and screen renderers are separated under `src/bin/ui/`.
- GPS snapshots feed the status bar and live navigation screen. The broadcast conversation remains a permanent entry in the conversation list.
- Nearby peers are learned passively from valid packets and actively through a user-triggered `DeviceAnnounce`/`DeviceAnnounceResponse` scan. Discovery IDs are public and unauthenticated.
- A discovered unpaired peer can receive a clearly warned plaintext introduction. Pairing uses the locally entered shared secret, canonical public IDs, authenticated key confirmation, and a persisted contact key.
- Paired direct text is encrypted per fragment with AES-256-GCM. The composer accepts up to 512 bytes, splits text into no more than 16 band-sized fragments, and the receiver authenticates/reassembles before delivering one chat item.
- Accepted direct text and coordinate messages receive a targeted ACK carrying the logical message ID. Only a matching encrypted direct-message row displays a green check; the indicator is volatile and means queue acceptance, not display or persistence.
- Received `Coordinates` packets display as selectable GPS rows. Up/Down selects a row and Enter opens distance/turn guidance; direction is estimated from successive local GPS fixes after five metres of movement.
- A fake navigator is enabled by default because the physical button expander is not integrated. Disable it for normal operation with `cargo run --release --no-default-features`.

Chat send/receive and location navigation are connected to the UI, but conversation history is still bounded in RAM and is lost on restart. The app does not currently expose a control to transmit its own GPS position, does not automatically retry packets, and has no physical button input task. See [Storage and identity](storage-and-identity.md#saving-and-loading-messages) for the unintegrated history API and [Chat, fragmentation, and GPS navigation](chat-fragmentation-and-navigation.md) for protocol and navigation limits.

## Hardware
- Display: ILI9341 2.4" SPI TFT
- Pins: GPIO12 SCK, GPIO11 MOSI, GPIO10 CS, GPIO9 D/C, GPIO8 RESET, GPIO7 BL
- SPI: display uses hardware SPI3 at 10 MHz; LR1121 remains on SPI2
- Driver: `mipidsi` + `embedded-graphics`; `embedded-hal-bus` wraps the SPI device

## Layout
- Top bar is persistent - shows GPS coords/time/fix status, updated from GPS snapshot via `embassy_sync::watch` from `gps_task`.
- The display has no keyboard. Text entry is intended for an external keyboard; the current fake navigator injects demo key events so screens can be previewed.
- UI state is held in `ui::state::UiState`; physical input controls are not wired into it yet.

## GPS
- GPS polling stays enabled. Disabling it would save little power on this board and would remove location/time features.
- `gps_task` polls the receiver and publishes snapshots through `embassy_sync::watch` for the top bar.
- The top bar shows `NO GPS FIX` or the latest coordinates/time.

## Boot/Menu Flow
```
MAIN MENU
├── CHATS  -> merged chats list
├── CONTACTS -> saved direct contacts + passive device discovery
└── SETTINGS
```

Settings opens Device, Frequency, and GPS categories. Each category contains selectable options; Enter opens a detail screen and Back returns to the previous level. The detail screens describe planned behavior and identify actions that are still previews.
The Device > Device Info screen shows the actual 16-byte identity loaded from flash at boot and the firmware version declared in `src/bin/main.rs`. If identity loading or creation failed, the screen reports that it is unavailable.
Contacts is separate from Settings. It lists saved direct contacts, passively heard announcements, and a one-shot action to announce this device. Listening runs in the background; opening the peer list does not transmit. A received `DeviceAnnounce` exposes the sender ID and signals that the peer chose to be discoverable. Selecting one offers an unencrypted DM after a warning. This does not save the peer as a contact. Pairing, encryption, and saving discovered devices are not wired up yet.

## Chats (Merged Message + Send)
**No separate SEND screen** - everything is in one chat view.

### CHATS List
```
CHATS
> BROADCAST        [PUBLIC]
  Alice            "see ya later"   14:32
  Bob              "got it"         13:58
  UNKNOWN AB12...   "?"              12:41
```

- **Broadcast** is a permanent, undeletable conversation contact at the top of the same contact list. It is unencrypted, uses `to = [0x00;16]`, and is clearly labeled `[PUBLIC - UNENCRYPTED]`.
- Known contacts show `custom_name` if set, else shortened device ID
- Unknown senders can appear if they messaged - can add to contacts/block from chat menu
- Sorted by last activity (most recent first)

### Chat View (per conversation)
```
+----------------------------------+
| < | Alice / BROADCAST            |  <- back, name/id
+----------------------------------+
|                                  |
| THEM: hey                        |
| YOU:  yep                        |
| THEM: [COORDS] 59.9139, 10.7522  |
|                                  |
+----------------------------------+
| [TYPE MESSAGE.............] ... |  <- input and message actions
+----------------------------------+
```

- UP/DOWN to scroll history
- Type using 5x5 grid, arrows to navigate cursor
- ENTER to send/focus input
- The three-dot menu (⋮) is beside the text input, not in the chat title bar. Activating it opens message actions such as SEND COORDS.

### 3-dot Menu (in text input)
```
┌─────────────────────────┐
│ SEND COORDS             │
│ CANCEL                  │
└─────────────────────────┘
```

- **SEND COORDS**: Pull from GPS snapshot. If no fix → toast "NO GPS FIX". Build coords payload (lat/lon as f32 LE, 8 bytes). Send with same encryption rules as message (encrypted if DM to known contact, unencrypted if broadcast). Display as `[COORDS] lat, lon` in chat.
- **CANCEL**: Close menu

## Encryption Rules
- **Broadcast** (`ConversationKind::Broadcast`, `to=[0x00;16]`): `is_encrypted = false`, plaintext payload
- **DM to known contact**: `is_encrypted = true`, encrypt with contact's key via `crypto::encrypt`. Each recipient gets separate packet if sending to multiple (own nonce/tag)
- **DM to unknown**: Don't allow encrypted sending unless added to contacts first. Can read but send disabled or prompt to add
- Payload format when encrypted: `nonce (12) || ciphertext || tag (16)`

## Message Storage Plan
- Use the existing `StorageManager::append_messages` / `append_broadcast_messages` APIs to save one `StoredMessage` per logical message.
- Use `from_me = true` for locally created messages and `false` for received messages.
- Load the active conversation through the paged APIs; avoid loading all history into RAM.
- Save received messages only after complete validation, reassembly and decryption.
- A 50-message cap, pruning policy and unread-count persistence are not implemented by the current storage API.

## Settings Structure
```
SETTINGS
├── Device
│   ├── Device Info (device ID and firmware version)
│   └── Reset Identity (confirm)
├── Frequency
│   └── Change frequency (show current, cycle; may require restart)
└── GPS
    └── Show Status
```

### Contact Discovery & Pairing
`Contacts > Listen for Announces` displays IDs from valid, unencrypted broadcast `DeviceAnnounce` packets. `Contacts > Announce My Device` queues one such packet with this device's ID in its `from` header. To discover each other, both devices can announce; one can also announce while the other only listens. A peer can be messaged without being saved, but the direct message is plaintext and requires explicit confirmation before each send. Do not send secrets. Password-based pairing and encrypted messaging still need a secure key-agreement protocol and are not connected to the UI.

### Clicking device ID vs custom name
- In chat list: if contact has custom name → show it; else show shortened hex
- On unknown sender: show "UNKNOWN <short hex>", can press ENTER for menu: ADD TO CONTACTS | BLOCK | VIEW ID | BACK

## State Machine (Draft)
```rust
#[derive(Clone, Debug)]
enum Screen {
    MainMenu { index: usize },
    ChatsList { index: usize },
    ChatView {
        kind: ConversationKind,
        scroll: usize,
        input: heapless::String<200>,
        cursor: usize,
        menu_open: bool,
    },
    Settings { index: usize },
    SettingsDevice,
    SettingsContacts { index: usize, mode: ContactsMode },
    SettingsFrequency,
    SettingsGps,
    Discovering { expires_at: u32, selected: usize },
    PasswordPrompt { target_device: [u8;16], input: heapless::String<64> },
}
```

## Remaining work

1. Integrate persistent chat-history append/read with the send/receive paths and define retention/pruning behavior.
2. Add and validate the physical button-expander input task; the default fake navigator is only a logical input source.
3. Add an explicit user action for sharing the local GPS position if location transmission is desired.
4. Add user-facing errors for queue-full partial fragment sends and storage capacity when history persistence is integrated.
5. Consider retry policy and delivery-state persistence separately; an ACK check is not proof of display or storage.
6. Continue UI polish such as unread counts and additional contact controls.

## Code Layout
- TFT driver and board initialization helpers live in `src/bin/hardware/display.rs`.
- The ILI9341 uses hardware SPI3. Message and settings storage currently use the dedicated internal-flash partition.
- Conversation/contact entries and the permanent Broadcast contact live in `src/bin/ui/contact.rs`.
- Screen state and logical key handling live in `src/bin/ui/state.rs`.
- Rendering is split by screen under `src/bin/ui/render/` (`top_bar.rs`, `menu.rs`, `chat.rs`, and shared `style.rs`).

## Notes
- Don't implement groups yet (v2). Focus on solid DMs + broadcast first.
- Discovery (passive or announce/response) is not authentication; shared-secret pairing uses authenticated key confirmation and never sends the secret.
- GPS polling stays enabled; settings can show current fix and time status.
- Multiple recipients: send separate packets per contact (each encrypted with their key).
- Current conversation messages and ACK indicators are in-memory only; no chat records are written to flash yet.
