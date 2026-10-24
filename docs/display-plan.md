# Display UI: Current State and Plan

## Implemented now

- The ILI9341 TFT is initialized by `hardware/display.rs` using `mipidsi` and `embedded-graphics` over hardware SPI3 at 10 MHz.
- UI state, contact list, and renderers are split across `src/bin/ui/` rather than one large render module.
- GPS task publishes snapshots for the persistent top bar.
- The broadcast conversation is a permanent entry in the conversation list.
- Screen drawing and logical key-state transitions exist.

The display is not yet a usable chat terminal: there is no physical button input task, no message send integration, no conversation-history loading/rendering, and no message persistence calls. The current chat renderer displays a history placeholder. See [Storage and identity](storage-and-identity.md#saving-and-loading-messages) for the persistence integration pattern.

## Hardware
- Display: ILI9341 2.4" SPI TFT
- Pins: GPIO12 SCK, GPIO11 MOSI, GPIO10 CS, GPIO9 D/C, GPIO8 RESET, GPIO7 BL
- SPI: display uses hardware SPI3 at 10 MHz; LR1121 remains on SPI2
- Driver: `mipidsi` + `embedded-graphics`; `embedded-hal-bus` wraps the SPI device

## Layout
```
+----------------------------------+
| GPS: XX.XXXX,YY.YYYY | HH:MM | FX|  <- TOP BAR (always visible)
+----------------------------------+
|                                  |
|           CURRENT SCREEN         |
|                                  |
+----------------------------------+
| [1][2][3][4][5]                  |
| [6][7][8][9][0]                  |
| [Q][W][E][R][T]                  |
| [A][S][D][F][G]                  |
| [<-][^][v][->][ENT]              |  <- ARROW ROW (bottom)
+----------------------------------+
```

- Top bar is persistent - shows GPS coords/time/fix status, updated from GPS snapshot via `embassy_sync::watch` from `gps_task`.
- Bottom row is arrow keys (left/up/down/right/enter). Top 4 rows form the character grid for typing.
- UI state is held in `ui::state::UiState`; physical buttons are not wired into it yet.

## GPS
- GPS polling stays enabled. Disabling it would save little power on this board and would remove location/time features.
- `gps_task` polls the receiver and publishes snapshots through `embassy_sync::watch` for the top bar.
- The top bar shows `NO GPS FIX` or the latest coordinates/time.

## Boot/Menu Flow
```
MAIN MENU
├── CHATS  -> merged chats list
└── SETTINGS
```

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
│   ├── View Device ID (hex)
│   ├── Firmware Version
│   └── Reset Identity (confirm)
├── Contacts
│   ├── Broadcast Device ID  (send DeviceAnnounce)
│   ├── Search for Devices   (discover announces, prompt for shared password)
│   ├── View/Edit/Delete     (select contact -> edit name/delete)
│   └── Blacklist
├── Frequency
│   └── Change frequency (show current, cycle; may require restart)
└── GPS
    └── Show Status
```

### Contact Discovery & Pairing
**Broadcaster**: `Settings > Contacts > Broadcast Device ID` → queue `DeviceAnnounce` packet (broadcast, empty payload, `PacketType::DeviceAnnounce = 0x05`)

**Searcher**: `Settings > Contacts > Search for Devices` → discover mode for ~10-15s, collect announces (ignore self). Select device → prompt for shared password. Derive key with `crypto::derive_pair_key(password, &my_id, &peer_id)` (sort IDs, domain `communicator-pair-v1`). Save contact with derived key.

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

## Implementation Order
1. **Data layer** - integrate `ui/state.rs`, conversation/message types with storage
2. **GPS snapshot sharing** - `embassy_sync::watch` in `gps_task`, top bar display
3. **Navigation/Input** - Key mapping for 5x5 grid + arrows, screen stack
4. **Top bar** - Always render first
5. **ChatsList + ChatView** - Merged UI, broadcast as pseudo-convo
6. **3-dot menu + SEND COORDS** - Context menu in input area
7. **Contact discovery** - DeviceAnnounce send/receive, search UI, pairwise key derivation
8. **Flash message persistence** - Save/load logical sent and received messages; decide history cap and pruning policy
9. **Settings pages** - Device/Contacts/Frequency/GPS
10. **Polish** - Unread counts, toasts, blacklisting

## Code Layout
- TFT driver and board initialization helpers live in `src/bin/hardware/display.rs`.
- The ILI9341 uses hardware SPI3. Message and settings storage currently use the dedicated internal-flash partition.
- Conversation/contact entries and the permanent Broadcast contact live in `src/bin/ui/contact.rs`.
- Screen state and logical key handling live in `src/bin/ui/state.rs`.
- Rendering is split by screen under `src/bin/ui/render/` (`top_bar.rs`, `menu.rs`, `chat.rs`, and shared `style.rs`).

## Notes
- Don't implement groups yet (v2). Focus on solid DMs + broadcast first.
- Discovery uses existing announce mechanism, no key exchange over radio.
- GPS polling stays enabled; settings can show current fix and time status.
- Multiple recipients: send separate packets per contact (each encrypted with their key).
- Messages stored decrypted in flash for UI responsiveness.
